use mcproto_rs::{
    v1_16_3::RawPacket753,
    v1_17_0::RawPacket755,
};

use super::ConnectionVersion;

pub struct V753;
impl<'a> ConnectionVersion<'a> for V753 {
    type Protocol = RawPacket753<'a>;
}
pub struct V755;
impl<'a> ConnectionVersion<'a> for V755 {
    type Protocol = RawPacket755<'a>;
}
