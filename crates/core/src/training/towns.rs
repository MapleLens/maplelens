//! Which maps are towns, where EXP is a quest's and not training (pure).
//!
//! The minimap's title is all both platforms know of the map: the browser has no map data, and the Windows app's
//! (the map helper's, from maplestory.io) arrives a while after the map is read, when the first EXP in a town would
//! already have shown the card. So the towns are a list of names, compared as `map_name` compares readings: an OCR
//! slip or a Free Market room still names its town, and a hunting ground named after its town ("Henesys Hunting
//! Ground I", 69 at most against any town) does not. A map not listed is a field, so an unread or unknown name never
//! hides the card from a player training.

use super::map_name::same_place;

/// The towns of MapleStory Classic World and the buildings in them: the maps its data (maplestory.io, MCW) marks a
/// town that have no monsters. Left out: jump quests and events, which the data marks a town too but where EXP is
/// not a quest's ("The Forest of Patience", "Physical Fitness Test"). A new region's towns go here; `live_towns`
/// checks the list against the data and prints the town maps it lacks.
pub const TOWNS: [&str; 51] = [
    // Maple Island
    "Mushroom Town",
    "Mushroom Town - West Entrance",
    "Mushroom Town Townstreet",
    "Southperry",
    "Southperry Armor Store",
    "Amherst",
    "Amherst Townstreet",
    "Amherst Weapon Store",
    "Amherst Department Store",
    // Victoria Island
    "Lith Harbor",
    "Lith Harbor Armor Shop",
    "Lith Harbor Department Store",
    "Lith Harbor Weapon Shop",
    "Henesys",
    "Henesys Townstreet",
    "Henesys Town Hall",
    "Henesys Market",
    "Henesys Weapon Store",
    "Henesys Department Store",
    "Henesys Plastic Surgery",
    "Henesys Hair Salon",
    "Henesys Skin-Care",
    "Henesys Park",
    "Bowman Instructional School",
    "Pet-Walking Road",
    "Yellow Mushroom House",
    "Ellinia",
    "Ellinia Weapon Store",
    "Ellinia Department Store",
    "Magic Library",
    "The Circulation Desk",
    "Marr's Forest",
    "Ellinia Station",
    "Before Takeoff <To Orbis>",
    "Kerning City",
    "Kerning City Self-Defence Item Store",
    "Kerning City Pharmacy",
    "Kerning City Hair Salon",
    "Kerning City Repair Shop",
    "Kerning City Civic Center",
    "Thieves' Hideout",
    "Niora Hospital",
    "Backalley Semi-Basement",
    "Perion",
    "Perion Weapon Store",
    "Perion Department Store",
    "Warriors' Sanctuary",
    "Sleepywood",
    "Florina Beach",
    "Free Market Entrance",
    // Every room: the numeral is compared apart, and a name without one fits them all.
    "Free Market",
];

/// Is `map` (the minimap's title, as OCR read it) a town?
pub fn is_town(map: &str) -> bool {
    TOWNS.iter().any(|town| same_place(town, map))
}

#[cfg(test)]
mod tests {
    use super::is_town;

    #[test]
    fn towns_and_their_buildings_are_towns() {
        for map in [
            "Kerning City",
            "Kernlng City", // OCR's look-alikes of I
            "Henesys",
            "Henesys Hair Salon",
            "Free Market <3>",
            "Free Market <17>",
            "Free Market", // OCR left the room out
            "Free Market Entrance",
            "Ell1nia",
            "Sleepywood",
        ] {
            assert!(is_town(map), "{map}");
        }
    }

    #[test]
    fn fields_named_after_their_town_are_not() {
        for map in [
            "Kerning City Middle Forest I",
            "Construction Site North of Kerning City",
            "Henesys Hunting Ground I",
            "The Field East of Amherst",
            "East Entrance to Mushroom Town", // monsters at a town's gate
            "Mushroom Garden",
            "Ant Tunnel I",
            "1st Accompaniment <1st Stage>",
            "",
        ] {
            assert!(!is_town(map), "{map}");
        }
    }
}
