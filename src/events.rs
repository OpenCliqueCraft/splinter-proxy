use mcproto_rs::protocol::{
    HasPacketKind,
    PacketErr,
    RawPacket,
};

use crate::current::proto::{
    Packet755 as PacketLatest,
    Packet755Kind as PacketLatestKind,
    RawPacket755 as RawPacketLatest,
};

/// A packet that is lazily deserialized when the deserialized packet is accessed
pub struct LazyDeserializedPacket<'a> {
    raw_packet: Option<RawPacketLatest<'a>>,
    de_packet: Option<Result<PacketLatest, PacketErr>>,
}

impl<'a> LazyDeserializedPacket<'a> {
    /// Creates a new lazy packet from a raw packet
    pub fn from_raw_packet(raw_packet: RawPacketLatest<'a>) -> Self {
        Self {
            raw_packet: Some(raw_packet),
            de_packet: None,
        }
    }
    /// Creates a new lazy packet from an already deserialized packet
    pub fn from_packet(packet: PacketLatest) -> Self {
        Self {
            raw_packet: None,
            de_packet: Some(Ok(packet)),
        }
    }
    fn de(&mut self) {
        if self.de_packet.is_none() {
            if let Some(raw_packet) = self.raw_packet.as_ref() {
                self.de_packet = Some(raw_packet.deserialize());
            }
        }
    }
    /// Gets a mutable reference to the deserialized packet. Packet may be deserialized during this
    /// call
    pub fn packet(&mut self) -> Result<&mut PacketLatest, &mut PacketErr> {
        self.de();
        let res = self.de_packet.as_mut().unwrap();
        res.as_mut()
    }
    /// Returns ownership to the deserialized packet. Packet may be deserialized during this call
    pub fn into_packet(mut self) -> Result<PacketLatest, PacketErr> {
        self.de();
        self.de_packet.unwrap()
    }
    pub fn into_raw_packet(self) -> Option<RawPacketLatest<'a>> {
        if self.is_deserialized() {
            None
        } else {
            self.raw_packet
        }
    }
    /// Checks if this packet has an already deserialized version
    pub fn is_deserialized(&self) -> bool {
        self.de_packet.is_some()
    }
    /// Gets the kind of this packet
    pub fn kind(&self) -> PacketLatestKind {
        if let Some(raw_packet) = self.raw_packet.as_ref() {
            raw_packet.kind()
        } else {
            self.de_packet.as_ref().unwrap().as_ref().unwrap().kind()
        }
    }
}
