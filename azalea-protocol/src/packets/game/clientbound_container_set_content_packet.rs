/*
use azalea_buf::McBuf;
use azalea_inventory::ItemSlot;
use azalea_protocol_macros::ClientboundGamePacket;

#[derive(Clone, Debug, McBuf, ClientboundGamePacket)]
pub struct ClientboundContainerSetContentPacket {
    pub container_id: i8,
    #[var]
    pub state_id: u32,
    pub items: Vec<ItemSlot>,
    pub carried_item: ItemSlot,
}
*/

use azalea_buf::{McBufReadable, BufReadExt};
use azalea_inventory::ItemSlot;
use azalea_protocol_macros::ClientboundGamePacket;
use std::io::{self, Cursor};
use azalea_core::read::try_read_content;
#[derive(Clone, Debug, ClientboundGamePacket)]
pub struct ClientboundContainerSetContentPacket {
    pub container_id: i8,
    pub state_id: u32, 
    pub items: Vec<ItemSlot>,
    pub carried_item: ItemSlot,
}
impl McBufReadable for ClientboundContainerSetContentPacket {
    fn read_from(buf: &mut Cursor<&[u8]>) -> Result<Self, io::Error> {
        let container_id = i8::read_from(buf)?;
        let state_id = buf.read_var_u32()?;
        let items = match try_read_content::<Vec<ItemSlot>>(buf) {
            Ok(i) => i,
            Err(e) => {
                eprintln!(
                    eprintln!("[PATCH] ContainerSet error, ignoring content in GUI: {}", e);
                    state_id, e
                );
                Vec::new() 
            }
        };
        let carried_item = ItemSlot::read_from(buf)?;

        Ok(ClientboundContainerSetContentPacket {
            container_id,
            state_id,
            items,
            carried_item,
        })
    }
}
