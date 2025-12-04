use azalea_buf::McBuf;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;
////////////////
use std::{io::{self, Read, Write}, sync::Arc};
use azalea_buf::{McBufReadable, McBufWritable, BufReadError};
////////////////

#[derive(McBuf, Debug, Clone, Default, Eq, PartialEq)]
pub struct GameProfile {
    /// The UUID of the player.
    pub uuid: Uuid,
    /// The username of the player.
    pub name: String,
    pub properties: HashMap<String, ProfilePropertyValue>,
}

impl GameProfile {
    pub fn new(uuid: Uuid, name: String) -> Self {
        GameProfile {
            uuid,
            name,
            properties: HashMap::new(),
        }
    }
}

impl From<SerializableGameProfile> for GameProfile {
    fn from(value: SerializableGameProfile) -> Self {
        let mut properties = HashMap::new();
        for value in value.properties {
            properties.insert(
                value.name,
                ProfilePropertyValue {
                    value: value.value,
                    signature: value.signature,
                },
            );
        }
        Self {
            uuid: value.id,
            name: value.name,
            properties,
        }
    }
}

///////////////////////////////////////////
/*#[derive(McBuf, Debug, Clone, Eq, PartialEq)]
pub struct ProfilePropertyValue {
    pub value: String,
    pub signature: Option<String>,
}*/
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProfilePropertyValue {
    pub value: String,
    pub signature: Option<String>,
}

// Implementacja McBufReadable
impl McBufReadable for ProfilePropertyValue {
    fn read_from(buf: &mut impl Read) -> Result<Self, BufReadError> {
        // 1. Wczytujemy wartość (tekstura)
        let value = String::read_from(buf)?;
        
        // 2. Wczytujemy sygnaturę (Option<String> obsługuje format MC)
        let signature = Option::<String>::read_from(buf)?;

        // 3. Sprawdzamy limity *po* wczytaniu
        const MAX_VALUE_LEN: usize = 32767; // Domyślny limit protokołu
        const MAX_SIGNATURE_LEN: usize = 1024; // Limit, który chcesz narzucić

        if value.len() > MAX_VALUE_LEN {
            return Err(BufReadError::UnexpectedStringLength {
                len: value.len(),
                max: MAX_VALUE_LEN,
            });
        }
        
        if let Some(sig) = &signature {
            if sig.len() > MAX_SIGNATURE_LEN {
                return Err(BufReadError::UnexpectedStringLength {
                    len: sig.len(),
                    max: MAX_SIGNATURE_LEN,
                });
            }
        }

        Ok(ProfilePropertyValue { value, signature })
    }
}

// Implementacja McBufWritable (nie ma limitów, tylko zapis)
impl McBufWritable for ProfilePropertyValue {
    fn write_into(&self, buf: &mut impl Write) -> Result<(), io::Error> {
        self.value.write_into(buf)?;
        self.signature.write_into(buf)?;
        Ok(())
    }
}
////////////////////////////////////////

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableGameProfile {
    pub id: Uuid,
    pub name: String,
    pub properties: Vec<SerializableProfilePropertyValue>,
}

impl From<GameProfile> for SerializableGameProfile {
    fn from(value: GameProfile) -> Self {
        let mut properties = Vec::new();
        for (key, value) in value.properties {
            properties.push(SerializableProfilePropertyValue {
                name: key,
                value: value.value,
                signature: value.signature,
            });
        }
        Self {
            id: value.uuid,
            name: value.name,
            properties,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableProfilePropertyValue {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_game_profile() {
        let json = r#"{
            "id": "f1a2b3c4-d5e6-f7a8-b9c0-d1e2f3a4b5c6",
            "name": "Notch",
            "properties": [
                {
                    "name": "qwer",
                    "value": "asdf",
                    "signature": "zxcv"
                }
            ]
        }"#;
        let profile =
            GameProfile::from(serde_json::from_str::<SerializableGameProfile>(json).unwrap());
        assert_eq!(
            profile,
            GameProfile {
                uuid: Uuid::parse_str("f1a2b3c4-d5e6-f7a8-b9c0-d1e2f3a4b5c6").unwrap(),
                name: "Notch".to_string(),
                properties: {
                    let mut map = HashMap::new();
                    map.insert(
                        "qwer".to_string(),
                        ProfilePropertyValue {
                            value: "asdf".to_string(),
                            signature: Some("zxcv".to_string()),
                        },
                    );
                    map
                },
            }
        );
    }
}
