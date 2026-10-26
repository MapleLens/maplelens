//! The labelled text samples the bundled glyph atlases are built from: real, native-resolution captures in
//! `tests/fixtures` and the text they show. Add a sample here when a new capture shows letters the atlas lacks,
//! then run `cargo run -p maplelens-core --example build_atlas`.

/// Which font a sample shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Font {
    /// Tooltip titles: bold, white, about 16 px caps at the reference resolution.
    Title,
    /// The name plate under the Character Info portrait: thin white letters on grey.
    Name,
}

pub struct Sample {
    pub fixture: &'static str,
    /// Where the tooltip (or, for names, the Character Info window) starts in the fixture.
    pub origin: (usize, usize),
    /// Title samples: how wide a strip the title is read from, and how many title lines.
    pub width: usize,
    pub lines: usize,
    pub text: &'static str,
    pub font: Font,
}

const fn title(
    fixture: &'static str,
    origin: (usize, usize),
    width: usize,
    lines: usize,
    text: &'static str,
) -> Sample {
    Sample { fixture, origin, width, lines, text, font: Font::Title }
}

pub const SAMPLES: &[Sample] = &[
    title("tooltip_native_over_sky.png", (0, 0), 450, 1, "Emerald Ore"),
    title("tooltip_native_over_window.png", (0, 0), 450, 1, "Emerald Ore"),
    title("tooltip_native_octopus_leg.png", (0, 0), 450, 1, "Octopus Leg"),
    title("tooltip_native_silver_ore.png", (0, 0), 450, 1, "Silver Ore"),
    title("tooltip_squishy_liquid.png", (31, 33), 450, 1, "Squishy Liquid"),
    title("tooltip_mushroom_spore_busy_backdrop.png", (0, 0), 450, 1, "Mushroom Spore"),
    title("tooltip_cash_two_line_title_ink_wash_ring.png", (0, 0), 379, 2, "Ink-and-Wash Painting Name Tag Ring"),
    title("strip_pushed_up_cash_ring_b.png", (0, 7), 370, 2, "Ink-and-Wash Painting Name Tag Ring"),
    title("compare_zamadar_garnier_strip.png", (0, 0), 370, 1, "Beginner's Triangular Zamadar"),
    title("compare_zamadar_garnier_strip.png", (373, 0), 370, 1, "Beginner's Garnier"),
    title("tooltip_equip_kitty_camping_hat.png", (0, 0), 370, 1, "Kitty Camping Hat"),
    title("tooltip_equip_shop_pants.png", (0, 0), 370, 1, "Lined Hip-Hop Pants"),
    // A player-crafted item: the crafter's name ("bTang's") takes the first line, the title sits 36 px lower.
    title("tooltip_equip_crafted_great_red_helmet.png", (0, 36), 370, 1, "Great Red Helmet"),
    // Title rows (450x70 crops) of real tooltips over all kinds of backdrops, chosen to teach the letters the samples
    // above lack (A D F J K Y b f w x z : . 5) a few times each.
    title("title_arrows_for_bows.png", (0, 0), 370, 1, "Arrows for Bows"),
    title("title_axe_pole_arm.png", (0, 0), 370, 1, "Axe Pole Arm"),
    title("title_blue_axe.png", (0, 0), 370, 1, "Blue Axe"),
    title("title_blue_jester.png", (0, 0), 370, 1, "Blue Jester"),
    title("title_bronze_football_helmet.png", (0, 0), 370, 1, "Bronze Football Helmet"),
    title("title_bronze_ore.png", (0, 0), 370, 1, "Bronze Ore"),
    title("title_brown_distinction.png", (0, 0), 370, 1, "Brown Distinction"),
    title("title_dark_guise.png", (0, 0), 370, 1, "Dark Guise"),
    title("title_diamond.png", (0, 0), 370, 1, "Diamond"),
    title("title_fireman_s_axe.png", (0, 0), 370, 1, "Fireman's Axe"),
    title("title_firewood.png", (0, 0), 370, 1, "Firewood"),
    title("title_fragment_of_magic.png", (0, 0), 370, 1, "Fragment of Magic"),
    title("title_hat_accuracy_scroll_lesser.png", (0, 0), 370, 1, "Hat Accuracy Scroll: Lesser"),
    title("title_jr_necki_skin.png", (0, 0), 370, 1, "Jr. Necki Skin"),
    title("title_kitty_camping_hat.png", (0, 0), 370, 1, "Kitty Camping Hat"),
    title("title_pig_s_ribbon.png", (0, 0), 370, 1, "Pig's Ribbon"),
    title("title_pink_jester.png", (0, 0), 370, 1, "Pink Jester"),
    title("title_rainbow_knit.png", (0, 0), 370, 1, "Rainbow Knit"),
    title("title_return_scroll_to_kerning_city.png", (0, 0), 370, 1, "Return Scroll to Kerning City"),
    title("title_selective_5_slot_coupon.png", (0, 0), 370, 1, "Selective 5-slot Coupon"),
    title("title_shoes_speed_scroll_lesser.png", (0, 0), 370, 1, "Shoes Speed Scroll: Lesser"),
    title("title_spear_attack_scroll_greater.png", (0, 0), 370, 1, "Spear Attack Scroll: Greater"),
    title("title_stiff_feather.png", (0, 0), 370, 1, "Stiff Feather"),
    title("title_storage_room_5_slot_coupon.png", (0, 0), 370, 1, "Storage Room 5-slot Coupon"),
    title("title_topaz_ore.png", (0, 0), 370, 1, "Topaz Ore"),
    title("title_yellow_rainbow_sneaks.png", (0, 0), 370, 1, "Yellow Rainbow Sneaks"),
    title("title_yellow_spatula.png", (0, 0), 370, 1, "Yellow Spatula"),
    Sample {
        fixture: "character_info_masterball.png",
        origin: (41, 68),
        width: 0,
        lines: 1,
        text: "masterball",
        font: Font::Name,
    },
];
