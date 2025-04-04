// Automatically generated rust module for 'rpc_messages.proto' file

#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(unused_imports)]
#![allow(unknown_lints)]
#![allow(clippy::all)]
#![cfg_attr(rustfmt, rustfmt_skip)]


use quick_protobuf::{MessageInfo, MessageRead, MessageWrite, BytesReader, Writer, WriterBackend, Result};
use core::convert::{TryFrom, TryInto};
use quick_protobuf::sizeofs::*;
use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Request {
    pub RequestType: rpc_messages::mod_Request::OneOfRequestType,
}

impl<'a> MessageRead<'a> for Request {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(810) => msg.RequestType = rpc_messages::mod_Request::OneOfRequestType::hello(r.read_message::<rpc_messages::Hello>(bytes)?),
                Ok(818) => msg.RequestType = rpc_messages::mod_Request::OneOfRequestType::submit_share(r.read_message::<rpc_messages::SubmitShare>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Request {
    fn get_size(&self) -> usize {
        0
        + match self.RequestType {
            rpc_messages::mod_Request::OneOfRequestType::hello(ref m) => 2 + sizeof_len((m).get_size()),
            rpc_messages::mod_Request::OneOfRequestType::submit_share(ref m) => 2 + sizeof_len((m).get_size()),
            rpc_messages::mod_Request::OneOfRequestType::None => 0,
    }    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        match self.RequestType {            rpc_messages::mod_Request::OneOfRequestType::hello(ref m) => { w.write_with_tag(810, |w| w.write_message(m))? },
            rpc_messages::mod_Request::OneOfRequestType::submit_share(ref m) => { w.write_with_tag(818, |w| w.write_message(m))? },
            rpc_messages::mod_Request::OneOfRequestType::None => {},
    }        Ok(())
    }
}


            impl TryFrom<&[u8]> for Request {
                type Error=quick_protobuf::Error;

                fn try_from(buf: &[u8]) -> Result<Self> {
                    let mut reader = BytesReader::from_bytes(&buf);
                    Ok(Request::from_reader(&mut reader, &buf)?)
                }
            }
            
pub mod mod_Request {

use super::*;

#[derive(Debug, PartialEq, Clone)]
pub enum OneOfRequestType {
    hello(rpc_messages::Hello),
    submit_share(rpc_messages::SubmitShare),
    None,
}

impl Default for OneOfRequestType {
    fn default() -> Self {
        OneOfRequestType::None
    }
}

}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Response {
    pub ResponseType: rpc_messages::mod_Response::OneOfResponseType,
}

impl<'a> MessageRead<'a> for Response {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(1610) => msg.ResponseType = rpc_messages::mod_Response::OneOfResponseType::nack(r.read_message::<rpc_messages::Nack>(bytes)?),
                Ok(1618) => msg.ResponseType = rpc_messages::mod_Response::OneOfResponseType::ack(r.read_message::<rpc_messages::Ack>(bytes)?),
                Ok(1626) => msg.ResponseType = rpc_messages::mod_Response::OneOfResponseType::hello(r.read_message::<rpc_messages::Hello>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Response {
    fn get_size(&self) -> usize {
        0
        + match self.ResponseType {
            rpc_messages::mod_Response::OneOfResponseType::nack(ref m) => 2 + sizeof_len((m).get_size()),
            rpc_messages::mod_Response::OneOfResponseType::ack(ref m) => 2 + sizeof_len((m).get_size()),
            rpc_messages::mod_Response::OneOfResponseType::hello(ref m) => 2 + sizeof_len((m).get_size()),
            rpc_messages::mod_Response::OneOfResponseType::None => 0,
    }    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        match self.ResponseType {            rpc_messages::mod_Response::OneOfResponseType::nack(ref m) => { w.write_with_tag(1610, |w| w.write_message(m))? },
            rpc_messages::mod_Response::OneOfResponseType::ack(ref m) => { w.write_with_tag(1618, |w| w.write_message(m))? },
            rpc_messages::mod_Response::OneOfResponseType::hello(ref m) => { w.write_with_tag(1626, |w| w.write_message(m))? },
            rpc_messages::mod_Response::OneOfResponseType::None => {},
    }        Ok(())
    }
}


            impl TryFrom<&[u8]> for Response {
                type Error=quick_protobuf::Error;

                fn try_from(buf: &[u8]) -> Result<Self> {
                    let mut reader = BytesReader::from_bytes(&buf);
                    Ok(Response::from_reader(&mut reader, &buf)?)
                }
            }
            
pub mod mod_Response {

use super::*;

#[derive(Debug, PartialEq, Clone)]
pub enum OneOfResponseType {
    nack(rpc_messages::Nack),
    ack(rpc_messages::Ack),
    hello(rpc_messages::Hello),
    None,
}

impl Default for OneOfResponseType {
    fn default() -> Self {
        OneOfResponseType::None
    }
}

}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Hello {
    pub my_height: u64,
    pub my_difficulty: u64,
}

impl<'a> MessageRead<'a> for Hello {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.my_height = r.read_uint64(bytes)?,
                Ok(16) => msg.my_difficulty = r.read_uint64(bytes)?,
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Hello {
    fn get_size(&self) -> usize {
        0
        + if self.my_height == 0u64 { 0 } else { 1 + sizeof_varint(*(&self.my_height) as u64) }
        + if self.my_difficulty == 0u64 { 0 } else { 1 + sizeof_varint(*(&self.my_difficulty) as u64) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.my_height != 0u64 { w.write_with_tag(8, |w| w.write_uint64(*&self.my_height))?; }
        if self.my_difficulty != 0u64 { w.write_with_tag(16, |w| w.write_uint64(*&self.my_difficulty))?; }
        Ok(())
    }
}


            impl TryFrom<&[u8]> for Hello {
                type Error=quick_protobuf::Error;

                fn try_from(buf: &[u8]) -> Result<Self> {
                    let mut reader = BytesReader::from_bytes(&buf);
                    Ok(Hello::from_reader(&mut reader, &buf)?)
                }
            }
            
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct SubmitShare {
    pub nonce: u64,
}

impl<'a> MessageRead<'a> for SubmitShare {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(8) => msg.nonce = r.read_uint64(bytes)?,
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for SubmitShare {
    fn get_size(&self) -> usize {
        0
        + if self.nonce == 0u64 { 0 } else { 1 + sizeof_varint(*(&self.nonce) as u64) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.nonce != 0u64 { w.write_with_tag(8, |w| w.write_uint64(*&self.nonce))?; }
        Ok(())
    }
}


            impl TryFrom<&[u8]> for SubmitShare {
                type Error=quick_protobuf::Error;

                fn try_from(buf: &[u8]) -> Result<Self> {
                    let mut reader = BytesReader::from_bytes(&buf);
                    Ok(SubmitShare::from_reader(&mut reader, &buf)?)
                }
            }
            
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Nack { }

impl<'a> MessageRead<'a> for Nack {
    fn from_reader(r: &mut BytesReader, _: &[u8]) -> Result<Self> {
        r.read_to_end();
        Ok(Self::default())
    }
}

impl MessageWrite for Nack { }


            impl TryFrom<&[u8]> for Nack {
                type Error=quick_protobuf::Error;

                fn try_from(buf: &[u8]) -> Result<Self> {
                    let mut reader = BytesReader::from_bytes(&buf);
                    Ok(Nack::from_reader(&mut reader, &buf)?)
                }
            }
            
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Ack { }

impl<'a> MessageRead<'a> for Ack {
    fn from_reader(r: &mut BytesReader, _: &[u8]) -> Result<Self> {
        r.read_to_end();
        Ok(Self::default())
    }
}

impl MessageWrite for Ack { }


            impl TryFrom<&[u8]> for Ack {
                type Error=quick_protobuf::Error;

                fn try_from(buf: &[u8]) -> Result<Self> {
                    let mut reader = BytesReader::from_bytes(&buf);
                    Ok(Ack::from_reader(&mut reader, &buf)?)
                }
            }
            
