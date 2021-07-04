use mcproto_rs::protocol::{
    HasPacketKind,
    PacketErr,
    RawPacket,
};

use crate::protocol::ConnectionVersion;

/// A packet that is lazily deserialized when the deserialized packet is accessed
pub struct LazyDeserializedPacket<'a, T>
where
    T: ConnectionVersion<'a>,
{
    raw_packet: Option<T::Protocol>,
    de_packet: Option<
        Result<<<T as ConnectionVersion<'a>>::Protocol as RawPacket<'a>>::Packet, PacketErr>,
    >,
}

impl<'a, T> LazyDeserializedPacket<'a, T>
where
    T: ConnectionVersion<'a>,
{
    /// Creates a new lazy packet from a raw packet
    pub fn from_raw_packet(raw_packet: T::Protocol) -> Self {
        Self {
            raw_packet: Some(raw_packet),

            de_packet: None,
        }
    }
    /// Creates a new lazy packet from an already deserialized packet
    pub fn from_packet(packet: <T::Protocol as RawPacket<'a>>::Packet) -> Self {
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
    pub fn packet(
        &mut self,
    ) -> Result<&mut <T::Protocol as RawPacket<'a>>::Packet, &mut PacketErr> {
        self.de();
        let res = self.de_packet.as_mut().unwrap();
        res.as_mut()
    }
    /// Returns ownership to the deserialized packet. Packet may be deserialized during this call
    pub fn into_packet(mut self) -> Result<<T::Protocol as RawPacket<'a>>::Packet, PacketErr> {
        self.de();
        self.de_packet.unwrap()
    }
    pub fn into_raw_packet(self) -> Option<T::Protocol> {
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
}

impl<'a, T> LazyDeserializedPacket<'a, T>
where
    T: ConnectionVersion<'a>,
    <<T as ConnectionVersion<'a>>::Protocol as RawPacket<'a>>::Packet:
        HasPacketKind<Kind = <<T as ConnectionVersion<'a>>::Protocol as HasPacketKind>::Kind>,
{
    /// Gets the kind of this packet
    pub fn kind(&self) -> <T::Protocol as HasPacketKind>::Kind {
        if let Some(raw_packet) = self.raw_packet.as_ref() {
            raw_packet.kind()
        } else {
            self.de_packet.as_ref().unwrap().as_ref().unwrap().kind()
        }
    }
}
