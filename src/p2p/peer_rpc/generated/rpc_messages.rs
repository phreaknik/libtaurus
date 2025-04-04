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
pub struct Request { }

impl<'a> MessageRead<'a> for Request {
    fn from_reader(r: &mut BytesReader, _: &[u8]) -> Result<Self> {
        r.read_to_end();
        Ok(Self::default())
    }
}

impl MessageWrite for Request { }


            impl TryFrom<&[u8]> for Request {
                type Error=quick_protobuf::Error;

                fn try_from(buf: &[u8]) -> Result<Self> {
                    let mut reader = BytesReader::from_bytes(&buf);
                    Ok(Request::from_reader(&mut reader, &buf)?)
                }
            }
            
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Response {
    pub payload: rpc_messages::mod_Response::OneOfpayload,
}

impl<'a> MessageRead<'a> for Response {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(1610) => msg.payload = rpc_messages::mod_Response::OneOfpayload::nack(r.read_message::<rpc_messages::Nack>(bytes)?),
                Ok(1618) => msg.payload = rpc_messages::mod_Response::OneOfpayload::ack(r.read_message::<rpc_messages::Ack>(bytes)?),
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
        + match self.payload {
            rpc_messages::mod_Response::OneOfpayload::nack(ref m) => 2 + sizeof_len((m).get_size()),
            rpc_messages::mod_Response::OneOfpayload::ack(ref m) => 2 + sizeof_len((m).get_size()),
            rpc_messages::mod_Response::OneOfpayload::None => 0,
    }    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        match self.payload {            rpc_messages::mod_Response::OneOfpayload::nack(ref m) => { w.write_with_tag(1610, |w| w.write_message(m))? },
            rpc_messages::mod_Response::OneOfpayload::ack(ref m) => { w.write_with_tag(1618, |w| w.write_message(m))? },
            rpc_messages::mod_Response::OneOfpayload::None => {},
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
pub enum OneOfpayload {
    nack(rpc_messages::Nack),
    ack(rpc_messages::Ack),
    None,
}

impl Default for OneOfpayload {
    fn default() -> Self {
        OneOfpayload::None
    }
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
            
