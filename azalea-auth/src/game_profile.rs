use std::{
    io::{self, Read, Write},
    sync::Arc,
};

// Zmienione importy na nowe nazwy (McBuf)
use azalea_buf::{
    BufReadError, McBuf, McBufReadable, McBufVarReadable, McBufVarWritable, McBufWritable,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use uuid::Uuid;

/// Information about the player that's usually stored on Mojang's servers.
#[derive(Debug, Clone, Default, Eq, PartialEq, McBuf)]
pub struct GameProfile {
    pub uuid: Uuid,
    pub name: String,
    pub properties: Arc<GameProfileProperties>,
}

impl GameProfile {
    pub fn new(uuid: Uuid, name: String) -> Self {
        GameProfile {
            uuid,
            name,
            properties: Arc::new(GameProfileProperties::default()),
        }
    }
}

impl From<SerializableGameProfile> for GameProfile {
    fn from(value: SerializableGameProfile) -> Self {
        Self {
            uuid: value.id.unwrap_or_default(),
            name: value.name.unwrap_or_default(),
            properties: Arc::new(value.properties.into()),
        }
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct GameProfileProperties {
    pub map: IndexMap<String, ProfilePropertyValue>,
}

// Implementacja McBufReadable (zamiast AzaleaRead)
impl McBufReadable for GameProfileProperties {
    fn read_from(buf: &mut impl Read) -> Result<Self, BufReadError> {
        let mut properties = IndexMap::new();
        // Używamy var_read_from zamiast azalea_read_var
        let properties_len = u32::var_read_from(buf)?;

        if properties_len > 16 {
            return Err(BufReadError::UnexpectedStringLength {
                len: properties_len as usize,
                max: 16,
            });
        }

        for _ in 0..properties_len {
            // Standardowy odczyt stringa (limit jest zaszyty w formacie MC, zwykle 32767)
            let key = String::read_from(buf)?;
            if key.len() > 16 {
                 // Opcjonalnie: ręczne sprawdzanie limitu jeśli wymagane
            }
            let value = ProfilePropertyValue::read_from(buf)?;
            properties.insert(key, value);
        }
        Ok(GameProfileProperties { map: properties })
    }
}

// Implementacja McBufWritable (zamiast AzaleaWrite)
impl McBufWritable for GameProfileProperties {
    fn write_into(&self, buf: &mut impl Write) -> Result<(), io::Error> {
        (self.map.len() as u32).var_write_into(buf)?;
        for (key, value) in &self.map {
            key.write_into(buf)?;
            value.write_into(buf)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ProfilePropertyValue {
    pub value: String,
    pub signature: Option<String>,
}

impl McBufReadable for ProfilePropertyValue {
    fn read_from(buf: &mut impl Read) -> Result<Self, BufReadError> {
        let value = String::read_from(buf)?;
        // Option<String> automatycznie obsługuje format Minecrafta (boolean present + string)
        let signature = Option::<String>::read_from(buf)?;
        Ok(ProfilePropertyValue { value, signature })
    }
}

impl McBufWritable for ProfilePropertyValue {
    fn write_into(&self, buf: &mut impl Write) -> Result<(), io::Error> {
        self.value.write_into(buf)?;
        self.signature.write_into(buf)?;
        Ok(())
    }
}

// --- Poniżej bez zmian (kod do Serializacji JSON) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableGameProfile {
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "SerializableProfileProperties::is_empty")]
    pub properties: SerializableProfileProperties,
}

impl From<GameProfile> for SerializableGameProfile {
    fn from(value: GameProfile) -> Self {
        Self {
            id: Some(value.uuid),
            name: Some(value.name),
            properties: (*value.properties).clone().into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct SerializableProfileProperties {
    pub list: Vec<SerializableProfilePropertyValue>,
}

impl SerializableProfileProperties {
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableProfilePropertyValue {
    pub name: String,
    pub value: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

impl From<GameProfileProperties> for SerializableProfileProperties {
    fn from(value: GameProfileProperties) -> Self {
        let mut list = Vec::new();
        for (name, entry) in value.map {
            list.push(SerializableProfilePropertyValue {
                name,
                value: entry.value,
                signature: entry.signature,
            });
        }
        Self { list }
    }
}

impl From<SerializableProfileProperties> for GameProfileProperties {
    fn from(value: SerializableProfileProperties) -> Self {
        let mut map = IndexMap::new();
        for entry in value.list {
            map.insert(
                entry.name,
                ProfilePropertyValue {
                    value: entry.value,
                    signature: entry.signature,
                },
            );
        }
        Self { map }
    }
}

impl Serialize for GameProfile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let serializable = SerializableGameProfile::from(self.clone());
        serializable.serialize(serializer)
    }
}
