use azalea_buf::McBuf;
use azalea_protocol_macros::ServerboundGamePacket;
use uuid::Uuid;

#[derive(Clone, Debug, McBuf, ServerboundGamePacket)]
pub struct ServerboundResourcePackPacket {
    pub id: Uuid,
    pub action: Action,
}

#[derive(McBuf, Clone, Copy, Debug)]
pub enum Action {
    SuccessfullyLoaded = 0,
    Declined = 1,
    FailedDownload = 2,
    Accepted = 3,
    Downloaded = 4, // AZALEA PLUS
    InvalidUrl = 5,
    FailedReload = 6,
    Discarded = 7,
}
