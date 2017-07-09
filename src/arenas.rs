use ranges::Ranges;
use std::collections::{HashMap, HashSet};

struct Thing {
    ident: String,
    associated_things: HashMap<String, String>,
    properties: HashMap<String, String>,
}

impl Thing {
    pub fn new(ident: &str) -> Thing {
        Thing {
            ident: ident.to_owned(),
            associated_things: HashMap::new(),
            properties: HashMap::new(),
        }
    }

    pub fn associate_with(&mut self, thing_type: &str, thing_ident: &str) {
        self.associated_things
            .insert(thing_type.to_owned(), thing_ident.to_owned());
    }

    pub fn set_property(&mut self, property: &str, value: &str) {
        self.properties
            .insert(property.to_owned(), value.to_owned());
    }

    pub fn description<'a, F>(&self, f: &F, skip_things: &HashSet<String>) -> String
        where F: Fn(&str) -> &'a Thing
    {
        let mut skip_things = skip_things.clone();
        skip_things.insert(self.ident.clone());
        let mut subthings: Vec<String> = self.associated_things
            .values()
            .flat_map(|ident| if !skip_things.contains(ident) {
                          Some(format!("{}: {}", ident, f(ident).description(f, &skip_things)))
                      } else {
                          None
                      })
            .collect();
        let properties: Vec<String> = self.properties
            .iter()
            .map(|(name, value)| format!("{}: {}", name, value))
            .collect();
        subthings.extend(properties);
        format!("{{ {} }}", subthings.join(", "))
    }
}

struct Arena {
    isa: Thing,
    memory_ranges: Ranges,
}

impl Arena {
    pub fn new(ident: &str) -> Arena {
        Arena {
            isa: Thing::new(ident),
            memory_ranges: Ranges::new(),
        }
    }

    pub fn associate_with(&mut self, thing_type: &str, thing_ident: &str) {
        self.isa.associate_with(thing_type, thing_ident);
    }

    pub fn set_property(&mut self, property: &str, value: &str) {
        self.isa.set_property(property, value);
    }

    pub fn description<'a, F>(&self, f: &F) -> String
        where F: Fn(&str) -> &'a Thing
    {
        self.isa.description(f, &HashSet::new())
    }

    pub fn allocate_chunk(&mut self, start: u64, size: u64) {
        self.memory_ranges.add(start, size);
    }

    pub fn deallocate_chunk(&mut self, start: u64, size: u64) {
        self.memory_ranges.remove(start, size);
    }

    pub fn ranges(&self) -> &Ranges {
        &self.memory_ranges
    }
}

pub struct Arenas {
    arenas: HashMap<String, Arena>,
    things: HashMap<String, Thing>,
}

impl Arenas {
    pub fn new() -> Arenas {
        Arenas {
            arenas: HashMap::new(),
            things: HashMap::new(),
        }
    }

    fn get_arena_mut<'a>(&'a mut self, ident: &str) -> &'a mut Arena {
        self.arenas
            .entry(ident.to_owned())
            .or_insert_with(|| Arena::new(ident))
    }

    fn get_arena_already_exists<'a>(&'a self, ident: &str) -> &'a Arena {
        self.arenas.get(ident).expect("arena should exist")
    }

    fn get_thing_mut<'a>(&'a mut self, ident: &str) -> &'a mut Thing {
        self.things
            .entry(ident.to_owned())
            .or_insert_with(|| Thing::new(ident))
    }

    fn get_thing_already_exists<'a>(&'a self, ident: &str) -> &'a Thing {
        self.things.get(ident).expect("Thing should exist")
    }

    pub fn allocate_chunk(&mut self, ident: &str, start: u64, size: u64) {
        self.get_arena_mut(ident).allocate_chunk(start, size);
    }

    pub fn deallocate_chunk(&mut self, ident: &str, start: u64, size: u64) {
        self.get_arena_mut(ident).deallocate_chunk(start, size);
    }

    pub fn associate_thing_with_thing(&mut self,
                                      type1: &str,
                                      ident1: &str,
                                      type2: &str,
                                      ident2: &str) {
        self.get_thing_mut(ident1).associate_with(type2, ident2);
        self.get_thing_mut(ident2).associate_with(type1, ident1);
    }

    pub fn associate_arena_with_thing(&mut self, ident1: &str, type2: &str, ident2: &str) {
        self.get_arena_mut(ident1).associate_with(type2, ident2);
    }

    pub fn set_thing_property(&mut self, ident: &str, property_name: &str, property_value: &str) {
        self.get_thing_mut(ident).set_property(property_name, property_value);
    }

    pub fn arena_description(&mut self, ident: &str) -> String {
        self.get_arena_mut(ident);
        self.get_arena_already_exists(ident)
            .description(&|thing_ident| self.get_thing_already_exists(thing_ident))
    }

    pub fn arena_ranges<'a>(&'a self, ident: &str) -> Option<&'a Ranges> {
        self.arenas.get(ident).map(|arena| arena.ranges())
    }
}

#[test]
fn test_arenas() {
    let mut arenas = Arenas::new();
    arenas.associate_arena_with_thing("ArenaAllocator:0x1", "nsDisplayListBuilder", "nsDisplayListBuilder:0x2");
    arenas.set_thing_property("nsDisplayListBuilder:0x2", "URL", "browser.xul");
    assert_eq!(arenas.arena_description("ArenaAllocator:0x1"), "{ nsDisplayListBuilder:0x2: { URL: browser.xul } }");

    arenas.associate_thing_with_thing("PresShell", "PresShell:0x5", "nsPresArena", "nsPresArena:0x4");
    arenas.set_thing_property("PresShell:0x5", "url", "dl-test.html");
    arenas.associate_arena_with_thing("ArenaAllocator:0x3", "nsPresArena", "nsPresArena:0x8");
    arenas.associate_arena_with_thing("ArenaAllocator:0x3", "nsPresArena", "nsPresArena:0x4");
    assert_eq!(arenas.arena_description("ArenaAllocator:0x3"), "{ nsPresArena:0x4: { PresShell:0x5: { url: dl-test.html } } }");

    arenas.allocate_chunk("ArenaAllocator:0x3", 100, 30);
    arenas.allocate_chunk("ArenaAllocator:0x3", 140, 10);
    arenas.deallocate_chunk("ArenaAllocator:0x3", 110, 10);
    {
        let ranges = arenas.arena_ranges("ArenaAllocator:0x3").unwrap();
        assert!(ranges.contains(101));
        assert!(ranges.contains(109));
        assert!(!ranges.contains(110));
        assert!(!ranges.contains(119));
        assert!(ranges.contains(120));
        assert!(ranges.contains(140));
        assert!(ranges.contains(141));
        assert!(!ranges.contains(150));
    }
}
