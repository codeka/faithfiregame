use serde::{Deserialize, Serialize};

fn default_max_stack() -> u32 {
    1
}

/// Logical equipment slot occupied when an item is equipped on a character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipSlot {
    Head,
    Body,
    Legs,
    Feet,
    Hands,
    Back,
    MainHand,
    OffHand,
}

impl EquipSlot {
    pub const ALL: [Self; 8] = [
        Self::Head,
        Self::Body,
        Self::Legs,
        Self::Feet,
        Self::Hands,
        Self::Back,
        Self::MainHand,
        Self::OffHand,
    ];
}

/// Single modular character visual attachment populated by an equipped item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VisualAttachment {
    Skinned { slot: String, part_id: String },
    Socket { socket_id: String, part_id: String },
}

/// Equipment rules and character visual attachments triggered when an item is equipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemEquipSpec {
    pub slot: EquipSlot,
    #[serde(default)]
    pub two_handed: bool,
    #[serde(default)]
    pub visuals: Vec<VisualAttachment>,
    #[serde(default)]
    pub hides_categories: Vec<String>,
    #[serde(default)]
    pub bonus_bag_slots: u16,
}

impl ItemEquipSpec {
    /// Returns true if equipping `self` conflicts with already having `other` equipped.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        self.slot == other.slot
            || (self.slot == EquipSlot::MainHand
                && self.two_handed
                && other.slot == EquipSlot::OffHand)
            || (other.slot == EquipSlot::MainHand
                && other.two_handed
                && self.slot == EquipSlot::OffHand)
    }

    /// Returns true if `self` occupies `target_slot` directly or as a two-handed main-hand item.
    pub fn occupies_slot(&self, target_slot: EquipSlot) -> bool {
        self.slot == target_slot
            || (self.slot == EquipSlot::MainHand
                && self.two_handed
                && target_slot == EquipSlot::OffHand)
    }
}

/// Authored item definition loaded from `world/items/catalog.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default = "default_max_stack")]
    pub max_stack: u32,
    #[serde(default)]
    pub weight: f32,
    #[serde(default)]
    pub preview_glb: Option<String>,
    #[serde(default)]
    pub equip: Option<ItemEquipSpec>,
}

/// Catalog of item definitions available across simulation and authoring tools.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ItemCatalog {
    #[serde(default)]
    pub items: Vec<ItemDef>,
}

impl ItemCatalog {
    pub fn get(&self, id: &str) -> Option<&ItemDef> {
        self.items.iter().find(|item| item.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_catalog_lookup_and_roundtrips() {
        let catalog = ItemCatalog {
            items: vec![
                ItemDef {
                    id: "cryo_jumpsuit".to_owned(),
                    name: "Cryo Jumpsuit".to_owned(),
                    description: "Standard cryo-berth thermal suit.".to_owned(),
                    category: "Apparel".to_owned(),
                    max_stack: 1,
                    weight: 1.2,
                    preview_glb: None,
                    equip: Some(ItemEquipSpec {
                        slot: EquipSlot::Body,
                        two_handed: false,
                        visuals: vec![VisualAttachment::Skinned {
                            slot: "torso".to_owned(),
                            part_id: "torso_cryo_jumpsuit".to_owned(),
                        }],
                        hides_categories: Vec::new(),
                        bonus_bag_slots: 0,
                    }),
                },
                ItemDef {
                    id: "guard_rifle".to_owned(),
                    name: "Guard Carbine".to_owned(),
                    description: "Compact security carbine.".to_owned(),
                    category: "Weapon".to_owned(),
                    max_stack: 1,
                    weight: 3.6,
                    preview_glb: None,
                    equip: Some(ItemEquipSpec {
                        slot: EquipSlot::MainHand,
                        two_handed: true,
                        visuals: vec![VisualAttachment::Socket {
                            socket_id: "socket_hand_r".to_owned(),
                            part_id: "weapon_guard_rifle".to_owned(),
                        }],
                        hides_categories: Vec::new(),
                        bonus_bag_slots: 0,
                    }),
                },
                ItemDef {
                    id: "salvage_rig_pack".to_owned(),
                    name: "Salvage Rig Pack".to_owned(),
                    description: "Field utility pack.".to_owned(),
                    category: "Back".to_owned(),
                    max_stack: 1,
                    weight: 1.5,
                    preview_glb: None,
                    equip: Some(ItemEquipSpec {
                        slot: EquipSlot::Back,
                        two_handed: false,
                        visuals: vec![VisualAttachment::Socket {
                            socket_id: "socket_back".to_owned(),
                            part_id: "pack_salvage_rig".to_owned(),
                        }],
                        hides_categories: Vec::new(),
                        bonus_bag_slots: 5,
                    }),
                },
                ItemDef {
                    id: "ration_pack".to_owned(),
                    name: "Ration Pack".to_owned(),
                    description: "Emergency caloric bar.".to_owned(),
                    category: "Consumable".to_owned(),
                    max_stack: 20,
                    weight: 0.25,
                    preview_glb: Some("/world/assets/items/ration_pack.glb".to_owned()),
                    equip: None,
                },
            ],
        };

        assert_eq!(catalog.get("guard_rifle").map(|i| i.name.as_str()), Some("Guard Carbine"));
        assert_eq!(
            catalog
                .get("salvage_rig_pack")
                .and_then(|i| i.equip.as_ref())
                .map(|e| e.bonus_bag_slots),
            Some(5)
        );
        assert_eq!(
            catalog.get("ration_pack").and_then(|i| i.preview_glb.as_deref()),
            Some("/world/assets/items/ration_pack.glb")
        );
        assert!(catalog.get("missing").is_none());

        #[cfg(feature = "json-wire")]
        {
            let json = serde_json::to_string(&catalog).unwrap();
            let from_json: ItemCatalog = serde_json::from_str(&json).unwrap();
            assert_eq!(from_json, catalog);
        }
    }

    #[test]
    fn equip_spec_conflict_and_occupation_rules_handle_two_handed_weapons() {
        assert_eq!(EquipSlot::ALL.len(), 8);

        let one_handed = ItemEquipSpec {
            slot: EquipSlot::MainHand,
            two_handed: false,
            visuals: Vec::new(),
            hides_categories: Vec::new(),
            bonus_bag_slots: 0,
        };
        let two_handed = ItemEquipSpec {
            slot: EquipSlot::MainHand,
            two_handed: true,
            visuals: Vec::new(),
            hides_categories: Vec::new(),
            bonus_bag_slots: 0,
        };
        let off_hand = ItemEquipSpec {
            slot: EquipSlot::OffHand,
            two_handed: false,
            visuals: Vec::new(),
            hides_categories: Vec::new(),
            bonus_bag_slots: 0,
        };
        let body = ItemEquipSpec {
            slot: EquipSlot::Body,
            two_handed: false,
            visuals: Vec::new(),
            hides_categories: Vec::new(),
            bonus_bag_slots: 0,
        };

        assert!(!one_handed.conflicts_with(&off_hand));
        assert!(!off_hand.conflicts_with(&one_handed));
        assert!(one_handed.conflicts_with(&two_handed));
        assert!(two_handed.conflicts_with(&off_hand));
        assert!(off_hand.conflicts_with(&two_handed));
        assert!(!two_handed.conflicts_with(&body));

        assert!(one_handed.occupies_slot(EquipSlot::MainHand));
        assert!(!one_handed.occupies_slot(EquipSlot::OffHand));
        assert!(two_handed.occupies_slot(EquipSlot::MainHand));
        assert!(two_handed.occupies_slot(EquipSlot::OffHand));
        assert!(!off_hand.occupies_slot(EquipSlot::MainHand));
    }
}
