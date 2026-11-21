use serde::{Deserialize, Serialize};

fn default_max_stack() -> u32 {
    1
}

/// Optional character visual attachment triggered when an item is equipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemEquipBinding {
    Skinned { slot: String, part_id: String },
    Socket { socket_id: String, part_id: String },
}

/// Authored item definition loaded from `world/items/catalog.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub equip: Option<ItemEquipBinding>,
}

/// Catalog of item definitions available across simulation and authoring tools.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
                    equip: Some(ItemEquipBinding::Skinned {
                        slot: "torso".to_owned(),
                        part_id: "torso_cryo_jumpsuit".to_owned(),
                    }),
                },
                ItemDef {
                    id: "guard_rifle".to_owned(),
                    name: "Guard Carbine".to_owned(),
                    description: "Compact security carbine.".to_owned(),
                    category: "Weapon".to_owned(),
                    max_stack: 1,
                    equip: Some(ItemEquipBinding::Socket {
                        socket_id: "socket_hand_r".to_owned(),
                        part_id: "weapon_guard_rifle".to_owned(),
                    }),
                },
                ItemDef {
                    id: "ration_pack".to_owned(),
                    name: "Ration Pack".to_owned(),
                    description: "Emergency caloric bar.".to_owned(),
                    category: "Consumable".to_owned(),
                    max_stack: 20,
                    equip: None,
                },
            ],
        };

        assert_eq!(catalog.get("guard_rifle").map(|i| i.name.as_str()), Some("Guard Carbine"));
        assert!(catalog.get("missing").is_none());

        #[cfg(feature = "json-wire")]
        {
            let json = serde_json::to_string(&catalog).unwrap();
            let from_json: ItemCatalog = serde_json::from_str(&json).unwrap();
            assert_eq!(from_json, catalog);
        }
    }
}
