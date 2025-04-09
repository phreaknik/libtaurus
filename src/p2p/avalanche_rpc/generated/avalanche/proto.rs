// Automatically generated rust module for 'avalanche.proto' file

#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(unused_imports)]
#![allow(unknown_lints)]
#![allow(clippy::all)]
#![cfg_attr(rustfmt, rustfmt_skip)]


use quick_protobuf::{MessageInfo, MessageRead, MessageWrite, BytesReader, Writer, WriterBackend, Result};
use quick_protobuf::sizeofs::*;
use super::super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Request {
    pub RequestData: avalanche::proto::mod_Request::OneOfRequestData,
}

impl<'a> MessageRead<'a> for Request {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(2) => msg.RequestData = avalanche::proto::mod_Request::OneOfRequestData::get_block(r.read_message::<avalanche::proto::BlockID>(bytes)?),
                Ok(10) => msg.RequestData = avalanche::proto::mod_Request::OneOfRequestData::get_preference(r.read_message::<avalanche::proto::BlockID>(bytes)?),
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
        + match self.RequestData {
            avalanche::proto::mod_Request::OneOfRequestData::get_block(ref m) => 1 + sizeof_len((m).get_size()),
            avalanche::proto::mod_Request::OneOfRequestData::get_preference(ref m) => 1 + sizeof_len((m).get_size()),
            avalanche::proto::mod_Request::OneOfRequestData::None => 0,
    }    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        match self.RequestData {            avalanche::proto::mod_Request::OneOfRequestData::get_block(ref m) => { w.write_with_tag(2, |w| w.write_message(m))? },
            avalanche::proto::mod_Request::OneOfRequestData::get_preference(ref m) => { w.write_with_tag(10, |w| w.write_message(m))? },
            avalanche::proto::mod_Request::OneOfRequestData::None => {},
    }        Ok(())
    }
}

pub mod mod_Request {

use super::*;

#[derive(Debug, PartialEq, Clone)]
pub enum OneOfRequestData {
    get_block(avalanche::proto::BlockID),
    get_preference(avalanche::proto::BlockID),
    None,
}

impl Default for OneOfRequestData {
    fn default() -> Self {
        OneOfRequestData::None
    }
}

}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Response {
    pub ResponseData: avalanche::proto::mod_Response::OneOfResponseData,
}

impl<'a> MessageRead<'a> for Response {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(0) => msg.ResponseData = avalanche::proto::mod_Response::OneOfResponseData::error(r.read_enum(bytes)?),
                Ok(10) => msg.ResponseData = avalanche::proto::mod_Response::OneOfResponseData::block(r.read_message::<avalanche::proto::Block>(bytes)?),
                Ok(18) => msg.ResponseData = avalanche::proto::mod_Response::OneOfResponseData::preferred(r.read_message::<avalanche::proto::BlockID>(bytes)?),
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
        + match self.ResponseData {
            avalanche::proto::mod_Response::OneOfResponseData::error(ref m) => 1 + sizeof_varint(*(m) as u64),
            avalanche::proto::mod_Response::OneOfResponseData::block(ref m) => 1 + sizeof_len((m).get_size()),
            avalanche::proto::mod_Response::OneOfResponseData::preferred(ref m) => 1 + sizeof_len((m).get_size()),
            avalanche::proto::mod_Response::OneOfResponseData::None => 0,
    }    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        match self.ResponseData {            avalanche::proto::mod_Response::OneOfResponseData::error(ref m) => { w.write_with_tag(0, |w| w.write_enum(*m as i32))? },
            avalanche::proto::mod_Response::OneOfResponseData::block(ref m) => { w.write_with_tag(10, |w| w.write_message(m))? },
            avalanche::proto::mod_Response::OneOfResponseData::preferred(ref m) => { w.write_with_tag(18, |w| w.write_message(m))? },
            avalanche::proto::mod_Response::OneOfResponseData::None => {},
    }        Ok(())
    }
}

pub mod mod_Response {

use super::*;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Error {
    NOT_FOUND = 0,
}

impl Default for Error {
    fn default() -> Self {
        Error::NOT_FOUND
    }
}

impl From<i32> for Error {
    fn from(i: i32) -> Self {
        match i {
            0 => Error::NOT_FOUND,
            _ => Self::default(),
        }
    }
}

impl<'a> From<&'a str> for Error {
    fn from(s: &'a str) -> Self {
        match s {
            "NOT_FOUND" => Error::NOT_FOUND,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub enum OneOfResponseData {
    error(avalanche::proto::mod_Response::Error),
    block(avalanche::proto::Block),
    preferred(avalanche::proto::BlockID),
    None,
}

impl Default for OneOfResponseData {
    fn default() -> Self {
        OneOfResponseData::None
    }
}

}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct BlockID {
    pub hash: Vec<u8>,
}

impl<'a> MessageRead<'a> for BlockID {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(2) => msg.hash = r.read_bytes(bytes)?.to_owned(),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for BlockID {
    fn get_size(&self) -> usize {
        0
        + if self.hash.is_empty() { 0 } else { 1 + sizeof_len((&self.hash).len()) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if !self.hash.is_empty() { w.write_with_tag(2, |w| w.write_bytes(&**&self.hash))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Block {
    pub version: u32,
    pub difficulty: u64,
    pub miner: Vec<u8>,
    pub parents: Vec<Vec<u8>>,
    pub inputs: Vec<Vec<u8>>,
    pub time: Vec<u8>,
    pub nonce: u64,
}

impl<'a> MessageRead<'a> for Block {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(0) => msg.version = r.read_uint32(bytes)?,
                Ok(8) => msg.difficulty = r.read_uint64(bytes)?,
                Ok(18) => msg.miner = r.read_bytes(bytes)?.to_owned(),
                Ok(26) => msg.parents.push(r.read_bytes(bytes)?.to_owned()),
                Ok(34) => msg.inputs.push(r.read_bytes(bytes)?.to_owned()),
                Ok(42) => msg.time = r.read_bytes(bytes)?.to_owned(),
                Ok(48) => msg.nonce = r.read_uint64(bytes)?,
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Block {
    fn get_size(&self) -> usize {
        0
        + if self.version == 0u32 { 0 } else { 1 + sizeof_varint(*(&self.version) as u64) }
        + if self.difficulty == 0u64 { 0 } else { 1 + sizeof_varint(*(&self.difficulty) as u64) }
        + if self.miner.is_empty() { 0 } else { 1 + sizeof_len((&self.miner).len()) }
        + self.parents.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
        + self.inputs.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
        + if self.time.is_empty() { 0 } else { 1 + sizeof_len((&self.time).len()) }
        + if self.nonce == 0u64 { 0 } else { 1 + sizeof_varint(*(&self.nonce) as u64) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.version != 0u32 { w.write_with_tag(0, |w| w.write_uint32(*&self.version))?; }
        if self.difficulty != 0u64 { w.write_with_tag(8, |w| w.write_uint64(*&self.difficulty))?; }
        if !self.miner.is_empty() { w.write_with_tag(18, |w| w.write_bytes(&**&self.miner))?; }
        for s in &self.parents { w.write_with_tag(26, |w| w.write_bytes(&**s))?; }
        for s in &self.inputs { w.write_with_tag(34, |w| w.write_bytes(&**s))?; }
        if !self.time.is_empty() { w.write_with_tag(42, |w| w.write_bytes(&**&self.time))?; }
        if self.nonce != 0u64 { w.write_with_tag(48, |w| w.write_uint64(*&self.nonce))?; }
        Ok(())
    }
}

