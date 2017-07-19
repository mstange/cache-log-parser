# Gathering CPU Cache usage information

This is a tool that gives you insight about how well Gecko is using data in the CPU cache.

## Problem statement

I was investigating painting performance in Gecko, specifically the "display list building" phase of a paint. I knew that display list building in Gecko was memory bound, and I had a microbenchmark for display list building.

Now I wanted to answer the following questions:

 - How much data are we reading from memory into the CPU's cache during one "display list building" instance?
 - How much of that data are we reading into the cache multiple times because it gets evicted and then used again?
 - We're using an arena allocator for some of the data that we need during display list building; what percentage of the data that we're reading from memory comes from this arena?
 - How much of the data that we're reading from memory ends up actually being used? How much of the memory read bandwidth is "wasted" by bytes that end up not being accessed, but which were only read because they were inside the same cache line as bytes that were accessed?
 - What are the callstacks at which these memory reads get triggered?

I then added instrumentation to both cachegrind and Gecko that lets me answer these questions.

## Overview

This tool consists of three pieces: Two patches - one for valgrind / cachegrind and one for Gecko - and a command line tool. The patches cause lots of logging to be printed to stderr, which I pipe into a file. And the command line tool parses that output in order to produce the information that I'm interested in.

Here's how I run the whole thing:

 1. Create a Firefox profile that's set to restore all tabs on startup, load the testcase that I want to capture, and shut down that instance of Firefox. This doesn't measure anything, it's just a preparatory measure so that I have something that can run automatically.
 2. Launch a local Firefox build that contains the Gecko instrumentation patch, through a build of valgrind which contains the valgrind instrumentation patch, with the Firefox profile created in the previous step. Usually like this: `./Inst/bin/valgrind --tool=cachegrind --LL=2097152,8,64 --trace-children=yes --child-silent-after-fork=yes ~/code/mozilla/obj-x86_64-pc-linux-gnu/dist/bin/firefox -P cachegrind 2> ~/cache-logging.log`
    Here, `cachegrind` is the name of the Firefox profile created in step one, and `~/cache-logging.log` is the file where the logged information will end up.
 3. Wait for about 10 minutes, as Firefox goes through the stages of creating a window, loading about:home in that window, realizing it has a session to restore, loading my testcase from the restored session, and rendering a few frames of the animation in the testcase.
 4. Close the Firefox window and wait a few more minutes for Firefox to shut down completely.
 5. The log file at `~/cache-logging.log` is now about 10 GB big.
 6. Now I run the parsing / analysis tool on the log.

## Analysis
 
The log file contains output from multiple processes, and from both Gecko and cachegrind, interleaved. Luckily, the interleaving seems to happen at the line level - I haven't seen any interleaving occur within a single line. I'm not quite sure why that is, but I'm happy about it.

First, I need to find out the PID of the process that I'm interested in. So I run the `find-processes` subcommand:

```
$ cargo run --release find-processes ~/cache-logging.log
```

It prints out something like this:

```
Parent process: 8764 (138075024 log lines)
Primary content process: 8884 (43684270 log lines)
Other child processes:
 - 8890 (6250998 log lines)
 - 8793 (1099359 log lines)
 - 8770 (393856 log lines)
 - 8765 (38 log lines)
```

I'm interested in the primary content process (i.e. the child process with the largest amount of output in the log), and in this case its PID is `8884`.

Next, I need to know the range in the log during which a "build displaylist" phase happened that I'm interested in. So I run the `find-sections` subcommand, with the pid `8884`:

```
$ cargo run --release find-sections -p 8884 ~/cache-logging.log
```

Here's some of its output:

```
  - DisplayList section which read 496.38 kB
      - frome line 51560454 to line 51690722 (130268 lines total)
      - read 496384 bytes from memory into the LL cache in total
      - read 496384 bytes of unique address ranges into the cache
         => 0% overhead due to memory ranges that were read more than once
      - accessed 376629 bytes
         => 32% overhead from unused parts of cache lines

[...]

  - DisplayList section which read 4.47 MB
      - frome line 130198497 to line 130645414 (446917 lines total)
      - read 4465664 bytes from memory into the LL cache in total
      - read 4458752 bytes of unique address ranges into the cache
         => 0% overhead due to memory ranges that were read more than once
      - accessed 2670335 bytes
         => 67% overhead from unused parts of cache lines

  - DisplayList section which read 5.16 MB
      - frome line 132184857 to line 132777404 (592547 lines total)
      - read 5156032 bytes from memory into the LL cache in total
      - read 5130112 bytes of unique address ranges into the cache
         => 1% overhead due to memory ranges that were read more than once
      - accessed 3122285 bytes
         => 65% overhead from unused parts of cache lines

```

Here I would pick the last section and make a note of the range of line numbers during which this display list building instance happens in the log, i.e. lines 132184857 to 132777404.

Now that I have both the PID and the line range of an interesting section, I can run analyses on that section.

### Double reads

If this output showed me a high overhead "due to memory ranges that were read more than once", then I would run the `analyze-double-reads` command next, for the section identified in the previous step:

```
$ cargo run --release analyze-double-reads -p 8884 \
   -s 132184857 -e 132777404 ~/cache-logging.log
```

This gives me a breakdown by arena allocator:

```
Read 402 cache-line sized memory ranges at least twice.
    3 cache-line sized memory ranges were read 3 times (0%)
    399 cache-line sized memory ranges were read 2 times (0%)
    79756 cache-line sized memory ranges were read 1 time (99%)

Read 1446400 (28%) bytes outside any arena.
    3 cache-line sized memory ranges were read 3 times (0%)
    359 cache-line sized memory ranges were read 2 times (2%)
    21873 cache-line sized memory ranges were read 1 time (98%)

Read 1926784 bytes (37%) from arena ArenaAllocator:0x2fcedb48:
    { nsPresArena:0x2fcec750: { PresShell:0x2fcec720: { URL: https://bug1379694.bmoattachments.org/attachment.cgi?id=8885894 } } }

    40 cache-line sized memory ranges were read 2 times (0%)
    30026 cache-line sized memory ranges were read 1 time (100%)

Read 1782784 bytes (35%) from arena ArenaAllocator:0x1ffeffdbc0:
    { nsDisplayListBuilder:0x1ffeffdb80: { url: https://bug1379694.bmoattachments.org/attachment.cgi?id=8885894 } }

    27856 cache-line sized memory ranges were read 1 time (100%)

Read 64 bytes (0%) from arena ArenaAllocator:0x4a0bc00:
    {  }

    1 cache-line sized memory ranges were read 1 time (100%)
```

In addition, for each of the arenas, it picks a random top 5 of the cache line sized memory ranges that were read most frequently, and prints out callstacks for the reads and the evictions of that memory range. For example:

```
      * Read cache line at address 0x57ca8800 2 times:
          1 At line 132185431:

            nsLayoutUtils::PaintFrame(gfxContext*, nsIFrame*, nsRegion const&, unsigned int, nsDisplayListBuilderMode, nsLayoutUtils::PaintFrameFlags) (/home/mstange/code/mozilla/layout/base/nsLayoutUtils.cpp:3499)
            mozilla::PresShell::Paint(nsView*, nsRegion const&, unsigned int) (/home/mstange/code/mozilla/layout/base/PresShell.cpp:6477)
            nsViewManager::ProcessPendingUpdatesPaint(nsIWidget*) (/home/mstange/code/mozilla/view/nsViewManager.cpp:481)
            nsViewManager::ProcessPendingUpdatesForView(nsView*, bool) (/home/mstange/code/mozilla/view/nsViewManager.cpp:413)
            nsViewManager::ProcessPendingUpdates() (/home/mstange/code/mozilla/view/nsViewManager.cpp:1094)
            nsRefreshDriver::Tick(long, mozilla::TimeStamp) (/home/mstange/code/mozilla/layout/base/nsRefreshDriver.cpp:2049)
            [...]

            This cache line was subsequently evicted at line 132341113:

            nsDisplayItem::nsDisplayItem(nsDisplayListBuilder*, nsIFrame*, mozilla::ActiveScrolledRoot const*) (/home/mstange/code/mozilla/layout/painting/nsDisplayList.cpp:2614)
            nsDisplayItem::nsDisplayItem(nsDisplayListBuilder*, nsIFrame*) (/home/mstange/code/mozilla/layout/painting/nsDisplayList.cpp:2604)
            nsDisplayBackgroundColor::nsDisplayBackgroundColor(nsDisplayListBuilder*, nsIFrame*, nsRect const&, nsStyleBackground const*, unsigned int) (/home/mstange/code/mozilla/layout/painting/nsDisplayList.h:3327)
            nsDisplayBackgroundImage::AppendBackgroundItemsToTop(nsDisplayListBuilder*, nsIFrame*, nsRect const&, nsDisplayList*, bool, nsStyleContext*, nsRect const&, nsIFrame*) (/home/mstange/code/mozilla/layout/painting/nsDisplayList.cpp:3182)
            nsFrame::DisplayBackgroundUnconditional(nsDisplayListBuilder*, nsDisplayListSet const&, bool) (/home/mstange/code/mozilla/layout/generic/nsFrame.cpp:2100)
            nsFrame::DisplayBorderBackgroundOutline(nsDisplayListBuilder*, nsDisplayListSet const&, bool) (/home/mstange/code/mozilla/layout/generic/nsFrame.cpp:2124)
            nsBlockFrame::BuildDisplayList(nsDisplayListBuilder*, nsRect const&, nsDisplayListSet const&) (/home/mstange/code/mozilla/layout/generic/nsBlockFrame.cpp:6738)
            nsIFrame::BuildDisplayListForChild(nsDisplayListBuilder*, nsIFrame*, nsRect const&, nsDisplayListSet const&, unsigned int) (/home/mstange/code/mozilla/layout/generic/nsFrame.cpp:3258)
            DisplayLine(nsDisplayListBuilder*, nsRect const&, nsRect const&, nsLineList_iterator&, int, int&, nsDisplayListSet const&, nsBlockFrame*, mozilla::css::TextOverflow*) [clone .isra.787] (/home/mstange/code/mozilla/layout/generic/nsBlockFrame.cpp:6701)
            nsBlockFrame::BuildDisplayList(nsDisplayListBuilder*, nsRect const&, nsDisplayListSet const&) (/home/mstange/code/mozilla/layout/generic/nsBlockFrame.cpp:6780)
            nsIFrame::BuildDisplayListForChild(nsDisplayListBuilder*, nsIFrame*, nsRect const&, nsDisplayListSet const&, unsigned int) (/home/mstange/code/mozilla/layout/generic/nsFrame.cpp:2998)
            DisplayLine(nsDisplayListBuilder*, nsRect const&, nsRect const&, nsLineList_iterator&, int, int&, nsDisplayListSet const&, nsBlockFrame*, mozilla::css::TextOverflow*) [clone .isra.787] (/home/mstange/code/mozilla/layout/generic/nsBlockFrame.cpp:6701)
            nsBlockFrame::BuildDisplayList(nsDisplayListBuilder*, nsRect const&, nsDisplayListSet const&) (/home/mstange/code/mozilla/layout/generic/nsBlockFrame.cpp:6793)
            nsIFrame::BuildDisplayListForChild(nsDisplayListBuilder*, nsIFrame*, nsRect const&, nsDisplayListSet const&, unsigned int) (/home/mstange/code/mozilla/layout/generic/nsFrame.cpp:2998)
            nsCanvasFrame::BuildDisplayList(nsDisplayListBuilder*, nsRect const&, nsDisplayListSet const&) (/home/mstange/code/mozilla/layout/generic/nsCanvasFrame.cpp:590)
            nsIFrame::BuildDisplayListForChild(nsDisplayListBuilder*, nsIFrame*, nsRect const&, nsDisplayListSet const&, unsigned int) (/home/mstange/code/mozilla/layout/generic/nsFrame.cpp:3243)
            mozilla::ScrollFrameHelper::BuildDisplayList(nsDisplayListBuilder*, nsRect const&, nsDisplayListSet const&) (/home/mstange/code/mozilla/layout/generic/nsGfxScrollFrame.cpp:3515)
            nsIFrame::BuildDisplayListForChild(nsDisplayListBuilder*, nsIFrame*, nsRect const&, nsDisplayListSet const&, unsigned int) (/home/mstange/code/mozilla/layout/generic/nsFrame.cpp:2998)
            mozilla::ViewportFrame::BuildDisplayList(nsDisplayListBuilder*, nsRect const&, nsDisplayListSet const&) (/home/mstange/code/mozilla/layout/generic/ViewportFrame.cpp:65)
            nsIFrame::BuildDisplayListForStackingContext(nsDisplayListBuilder*, nsRect const&, nsDisplayList*) (/home/mstange/code/mozilla/layout/generic/nsFrame.cpp:2593)
            nsLayoutUtils::PaintFrame(gfxContext*, nsIFrame*, nsRegion const&, unsigned int, nsDisplayListBuilderMode, nsLayoutUtils::PaintFrameFlags) (/home/mstange/code/mozilla/layout/base/nsLayoutUtils.cpp:3594)
            mozilla::PresShell::Paint(nsView*, nsRegion const&, unsigned int) (/home/mstange/code/mozilla/layout/base/PresShell.cpp:6477)
            nsViewManager::ProcessPendingUpdatesPaint(nsIWidget*) (/home/mstange/code/mozilla/view/nsViewManager.cpp:481)
            nsViewManager::ProcessPendingUpdatesForView(nsView*, bool) (/home/mstange/code/mozilla/view/nsViewManager.cpp:413)
            nsViewManager::ProcessPendingUpdates() (/home/mstange/code/mozilla/view/nsViewManager.cpp:1094)
            nsRefreshDriver::Tick(long, mozilla::TimeStamp) (/home/mstange/code/mozilla/layout/base/nsRefreshDriver.cpp:2049)
            [...]

          2 At line 132776690:

            mozilla::PresShell::AddCanvasBackgroundColorItem(nsDisplayListBuilder&, nsDisplayList&, nsIFrame*, nsRect const&, unsigned int, unsigned int) (/home/mstange/code/mozilla/layout/base/PresShell.cpp:5298)
            nsLayoutUtils::PaintFrame(gfxContext*, nsIFrame*, nsRegion const&, unsigned int, nsDisplayListBuilderMode, nsLayoutUtils::PaintFrameFlags) (/home/mstange/code/mozilla/layout/base/nsLayoutUtils.cpp:3622)
            mozilla::PresShell::Paint(nsView*, nsRegion const&, unsigned int) (/home/mstange/code/mozilla/layout/base/PresShell.cpp:6477)
            nsViewManager::ProcessPendingUpdatesPaint(nsIWidget*) (/home/mstange/code/mozilla/view/nsViewManager.cpp:481)
            nsViewManager::ProcessPendingUpdatesForView(nsView*, bool) (/home/mstange/code/mozilla/view/nsViewManager.cpp:413)
            nsViewManager::ProcessPendingUpdates() (/home/mstange/code/mozilla/view/nsViewManager.cpp:1094)
            nsRefreshDriver::Tick(long, mozilla::TimeStamp) (/home/mstange/code/mozilla/layout/base/nsRefreshDriver.cpp:2049)
            [...]

            (No eviction)
```

### Cache line usage

The more interesting analysis for this particular run is the analysis of the "read bytes" vs "used bytes" vs "wasted bytes". This is done using the `generate-profiles` command:

```
$ cargo run --release generate-profiles -p 8884 \
   -s 132184857 -e 132777404 ~/cache-logging.log
```

This command creates three files: `read_bytes_profile.sps.json`, `used_bytes_profile.sps.json` and `wasted_bytes_profile.sps.json`, currently in the hardcoded location at `/home/mstange/Desktop/`. These are profiles in the [Gecko profile format](https://github.com/devtools-html/perf.html/blob/318f6b730b8396240519cd582599fd49715f89cb/docs/gecko-profile-format.md#source-data-format) and can be loaded into [perf.html](https://perf-html.io/). Here are the profiles from this example run:

 - [read_bytes](https://perfht.ml/2tc2in6)
 - [used_bytes](https://perfht.ml/2tc9ZcS)
 - [wasted_bytes](https://perfht.ml/2tczlaR)

So what do these profiles show?

Every "millisecond" (ms) in these profiles actually corresponds to 1KB of memory reads. Please keep this in mind; the UI only shows "ms" and "time" because that's what it was built to do, but now we're using it for displaying memory data instead of time data.

For example, in the read_bytes profile, the "total time" of 4830.0ms means that 4830.0KB of data were read from memory into the cache during display list building.

The call stacks shown in the call tree of the profile are the call stacks where the reads occurred.

In the used_bytes profile, the numbers describe how many bytes were actually accessed while a given cache line was in the cache, and the stack is the stack at which this cache line was read. For example, if the profile shows the number 678.5ms next to the call stack `nsDisplayListBuilder::AllocateDisplayItemClipChain`, this means that, of all the bytes that were read at `AllocateDisplayItemClipChain`, 678.5KB ended up being accessed before the corresponding cache line was evicted from the cache.

Keep in mind that some of these accesses (or even most of them) may have occurred long after the cache line has been read into the cache! And the profile does not contain call stacks for those subsequent memory accesses. The stacks that you see are really only the ones for the first access: the memory access which was responsible for reading that piece of memory into the cache.

The wasted_bytes profile shows the difference between read and used bytes. It answers the question: How many bytes of the ones that were read at a certain call stack ended up not being accessed while this piece of memory was in the cache? Or in other words, how many bytes of reads triggered by a certain call stack were wasted?

If the cache line size were one byte instead of 64 bytes, the wasted_bytes profile would always be empty.

#### Aside

You might be thinking that it would be more interesting to see a wastage percentage per call stack (e.g. this function wasted 80% of the bytes that it read), and then sort by that percentage. However, this might distort the importance of functions that are called rarely but waste a high percentage. I think sorting by wasted bytes is more likely to show high impact stacks at the top.

It would be nice to be able to generate one combined profile that just has different columns for "bytes read", "bytes used", "bytes wasted" and "% wasted", but that would require perf.html UI changes and profile format changes.

## Implementation

The Gecko instrumentations outputs the following information:

 - Start and end markers of the display list building phase:

 	```
 	==8884== Begin Display list building
 	[...]
 	==8884== End Display list building
 	```
 - Information about arenas:
 
 	```
    ==8884== [ArenaAllocator:0x2fcedb48] Allocating arena chunk at 0x1237b3000 with size 8192 bytes
    ==8884== [nsPresArena:0x2fcec750] has [ArenaAllocator:0x2fcedb48]
    ==8884== [PresShell:0x2fcec720] has [nsPresArena:0x2fcec750]
    ==8884== [PresShell:0x2fcec720] has URL https://bug1379694.bmoattachments.org/attachment.cgi?id=8885894
    ==8884== [ArenaAllocator:0x2fcedb48] Allocating arena chunk at 0x1243fc000 with size 8192 bytes
    [...]
    ==8884== [ArenaAllocator:0x2fcedb48] Deallocating arena chunk at 0x1243fc000 with size 8192 bytes
 	```
 - Information about shared libraries (so that callstacks can be symbolicated):

	```
    ==8884== SharedLibsChunk: [{"start": 1081344, "end": 1209608, "offset": 0, "name": "firefox", "path": "/home/mstange/code/mozilla/obj-x86_64-pc-linux-gnu/dist/bin/firefox", "debugName": "firefox", "debugPath": "/home/mstange/code/mozilla/obj-x86_64-pc-linux-gnu/dist/bin/firefox", "breakpadId": "DF884E2F6B7F2156B555BB3BECD5697E0", "arch": ""}, {"start": 67108864, "end": 69366120, "offset": 0, "name": "ld-linux-x86-64.so.2", "path": "/lib64/ld-linux-x86-64.so.2", "debugName": "ld-linux-x86-64.so.2", "debugPath": "/lib64/ld-linux-x86-6
    ==8884== SharedLibsChunk: 4.so.2", "breakpadId": "45F3AF02EE9C99199D01899D1325F03D0", "arch": ""}, {"start": 67272704, "end": 67293577, "offset": 0, "name": "libplc4.so", "path": "/home/mstange/code/mozilla/obj-x86_64-pc-linux-gnu/dist/bin/libplc4.so", "debugName": "libplc4.so", "debugPath": "/home/mstange/code/mozilla/obj-x86_64-pc-linux-gnu/dist/bin/libplc4.so", "breakpadId": "83EA728AFBECEFFF883A4D776AAB27250", "arch": ""}, {"start": 67297280, "end": 67313945, "offset": 0, "name": "libplds4.so", "path": "/home/mstange/code/mozill
    [...]
    ==8884== SharedLibsChunk: 4-linux-gnu/gconv/UTF-16.so", "debugName": "UTF-16.so", "debugPath": "/usr/lib/x86_64-linux-gnu/gconv/UTF-16.so", "breakpadId": "D52C8118785A060855AB2FBE69B113E10", "arch": ""}]

	```

The cachegrind instrumentation outputs the following information:

 - At the beginning, it prints out information about the LL cache.

    ```
    ==8884== LL cache information: 2097152 B, 64 B, 8-way associative
    ```

 - Whenever there's a cache miss, it prints out one or two `LLCacheSwapUB` lines ("cache line swap in the LL cache, with used bytes information"), one `LLMiss` line, and a `stack` line. The `stack` line may additionally be preceded by `add_frame` and `add_stack` lines.

 	The `LLCacheSwapUB` line mentions the address of the piece of the cache line sized piece of memory that gets put in the cache, the address of the piece of memory that gets evicted from the cache, the size of the cache line, and the number of bytes that have been used *of the cache line that is getting evicted*. In other words, for a given cache line, you only get the information about how much of it has been used at the point where it gets evicted from the cache because it's being replaced by something else, due to a memory read.

    The `LLMiss` line has more information about the read that triggered the cache miss. You get the exact address that we read from with the exact number of bytes that were read, and a reason ("why"). However, since these lines are only printed for the read that caused the cache miss, and not for subsequent reads of that cache line, it's not all that useful. I completely ignore the `LLMiss` lines at the moment.

    ```
    ==8884== LLCacheSwapUB: new_start=1ffeff82c0 old_start=5a1382c0 size=64 used_bytes=64
    ==8884== LLMiss: why=    D1 size=4 addr=1ffeff82f4 tid=1
    ==8884== stack: 20342
    ==8884== LLCacheSwapUB: new_start=1ffeff8380 old_start=5a178380 size=64 used_bytes=64
    ==8884== LLMiss: why=    D1 size=1 addr=1ffeff8380 tid=1
    ==8884== add_stack: 1377520 1377518 10292
    ==8884== add_stack: 1377521 1377520 8547
    ==8884== add_stack: 1377522 1377521 17957
    ==8884== stack: 1377522
    ==8884== LLCacheSwapUB: new_start=57866dc0 old_start=95e6dc0 size=64 used_bytes=8
    ==8884== LLMiss: why=    D1 size=1 addr=57866de0 tid=2
    ==8884== stack: 112164
    ==8884== Begin DisplayList building
    ==8884== LLCacheSwapUB: new_start=103942c0 old_start=5a1142c0 size=64 used_bytes=64
    ==8884== LLMiss: why=I1_NoX size=4 addr=103942e8 tid=18
    ==8884== stack: 245291
    ==8884== LLCacheSwapUB: new_start=10394300 old_start=5a114300 size=64 used_bytes=64
    ==8884== LLMiss: why=I1_Gen size=5 addr=103942fd tid=18
    ==8884== stack: 245292
    ==8884== LLCacheSwapUB: new_start=10393d00 old_start=5a113d00 size=64 used_bytes=64
    ==8884== LLMiss: why=I1_NoX size=1 addr=10393d30 tid=18
    ==8884== stack: 245294
    ==8884== LLCacheSwapUB: new_start=10393d40 old_start=5a053d40 size=64 used_bytes=64
    ==8884== LLMiss: why=I1_Gen size=7 addr=10393d3c tid=18
    ==8884== stack: 245294
    ```

### Cache simulation

Cachegrind's cache simulation is extremely simple and very easy to play with. All the code that's needed is contained [in the file cg_sim.c](https://github.com/svn2github/valgrind/blob/master/cachegrind/cg_sim.c).

Let's say your stack has these settings:

 - Total size: 2MB (= 2097152 bytes)
 - Cache line size: 64 bytes (this seems to be genenally true for x86 CPUs)
 - Associativity: 16

This cache can accomodate 2MB / 64B = 32768 cache lines.

For each of these cache lines, all we need to know is which address in memory is currently stored in that part of the cache. So we have an array of 32768 "tags", where each tag is a number that is computed as \<memory address> / 64.

The array of tags forms a 2D table which has 16 columns and 32768 / 16 = 2048 rows. The number of columns (here 16) is the cache associativity, and each row is called a "set".

Repeating the above: Our cache consists of 2048 sets, and each set contains 16 cache lines. 2048 * 16 * 64B = 2MB. For each cache line we save a tag which describes the memory address that is stored in that cache line.

A given memory address can only be cached in one set; the address determines the set number that it can be cached in. The set number is computed like this: (\<memory address> / 64) % 2048. For example, the memory at address 0x20000 (131072) can only be cached in set 0, and the memory at address 0x20040 (131136, i.e. 64 bytes to the right of 131072) can only be cached in set 1.

When a given memory address is accessed, the cache simulation does the following:

 - Compute the set number that this address can be cached in.
 - If the tag for that address is already in the set, mark it as the most recently used tag of that set. Cachegrind does this by reordering the tags in the set so that the most recently used one is at the front.
 - Otherwise: remove the least recently used cache line of this set, and insert the new tag as the most recently used one (i.e. at the front).

If a memory access straddles two cache lines, the steps above have to be followed for both cache lines.

### Used bytes

My cachegrind patch extends the simulation outlined above a little: Instead of only storing one tag per cache line, we also store a bitmask of the bytes that have been accessed of that tag.

The patch calls this a `used_tag`; the name `usage_bitmask` would probably have been a better choice. A `used_tag` is of type `ULong`, which is a 64bit unsigned integer. Luckily, cache lines are 64 bytes big and not bigger, so things fit nicely.

On every cache access, since we know the exact address and byte count of that access, we create a bitmask that has ones in the accessed bytes and zeros in the rest, and we "or" that bitmask into the existing `used_tag` (or insert it as a new `used_tag`). Reordering within the set happens in sync with the reordering of the regular tags.

Once a cache line gets evicted, we count the bits in the `used_tag` and write that count out in the `used_bytes` logging.

### Call stacks and symbolication

In order to keep the overhead of recording as low as possible, and in order to save space in the log, call stacks are recorded in a very succinct format. Instead of having a list of stack frames for each cache miss, we build up a tree of stack nodes and only reference the node index for a given call stack. As nodes are getting added to the tree, they're printed using `add_stack` log lines, so that the tree can be reconstructed by the log parser.

This means that we need to parse the whole log from the beginning even if we are only interested in cache information of a small section of the log, just so that we can build up a complete stack tree.

Symbolication is done by the log parsing tool. It maps a given stack frame address to the containing library, using the library table from the Gecko instrumentation, and then runs the command line tool `addr2line` in order to get the symbol name for the address.

It uses `addr2line --inline-frames`, so the resulting profiles contain stack frames even for functions that were inlined into other functions.

## How to run it

If you want to use this tool to get your own profiles, here's how to do it:

 1. Be on Linux (e.g. Ubuntu).
 2. Clone valgrind, apply `valgrind.diff`, and compile it:

    ```
    svn co svn://svn.valgrind.org/valgrind/trunk
  cd trunk
  (apply the patch)
  ./autogen.sh
  ./configure --prefix=`pwd`/Inst
  make -j8 && make -j8 install
    ```
 3. Apply `gecko.diff` to your mozilla-central clone and compile it with the following mozconfig options:

    ```
    TBD
    ```
 4. Make sure you have a good testcase for the problem you want to investigate.
 5. Run your patched cachegrind with your patched Firefox and pipe the stderr output to a file:

    ```
    ./Inst/bin/valgrind --tool=cachegrind --LL=2097152,8,64 \
        --trace-children=yes --child-silent-after-fork=yes \
        ~/code/mozilla/obj-x86_64-pc-linux-gnu/dist/bin/firefox -P cachegrind \
        2> ~/cache-logging.log
    ```

	This also overrides the size of the LL cache with something that's hopefully somewhat representative of regular machines.

## Next steps

Here's a list of features I'd like to add soon:

 - Generate separate profiles per arena

## Acknowledgements

Thanks Julian Seward for sending me a cachegrind patch that showed me the functions that needed to be instrumented.

And thanks to Julian Seward and Nicholas Nethercote for creating valgrind and cachegrind, without which I wouldn't have been able to get any of this information.

## FAQ

### How much did these findings improve performance?

I'm not at that stage yet. I've filed a few bugs but haven't written or tested any patches yet.

### Which cache are you simulating? Why the LL cache?

Regular cachegrind simulates multiple levels of cache, just as there are multiple caches in a CPU.

For simplicity, I only want to simulate one cache. So my cachegrind patch disables simulation of the other caches completely and uses the LL (last-level) cache as its only cache. Access latencies are not part of the simulation, so it doesn't really matter which cache you think this is; it's just a random cache and you can choose its size as you wish, using the `--LL` parameter when running valgrind.

The specific reason why I disabled the other caches is that I need information about what bytes are accessed, and I want to record that information for the LL cache, and if a certain memory access already hits an earlier cache then I wouldn't get a chance to record that information in the LL cache simulation.

### What about memory writes? You're only talking about reads.

This is a distinction that I've been ignoring, and I'm pretty sure that cachegrind's regular simulation ignores it as well. When a certain memory address is accessed, regardless of whether it's a read or a write, the simulation reads that piece of memory into the cache. A real CPU would sync back any writes to memory afterwards, but that behavior is not part of the simulation.

### What do the different cache miss reasons mean?

I don't know.
