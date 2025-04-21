// Automatically generated rust module for 'wire.proto' file

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
pub struct Broadcast {
    pub vertex: Option<generated::proto::Vertex>,
}

impl<'a> MessageRead<'a> for Broadcast {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(2) => msg.vertex = Some(r.read_message::<generated::proto::Vertex>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Broadcast {
    fn get_size(&self) -> usize {
        0
        + self.vertex.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.vertex { w.write_with_tag(2, |w| w.write_message(s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Request {
    pub RequestData: generated::proto::mod_Request::OneOfRequestData,
}

impl<'a> MessageRead<'a> for Request {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(2) => msg.RequestData = generated::proto::mod_Request::OneOfRequestData::get_block(r.read_message::<generated::proto::Hash>(bytes)?),
                Ok(10) => msg.RequestData = generated::proto::mod_Request::OneOfRequestData::get_vertex(r.read_message::<generated::proto::Hash>(bytes)?),
                Ok(18) => msg.RequestData = generated::proto::mod_Request::OneOfRequestData::get_preference(r.read_message::<generated::proto::Hash>(bytes)?),
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
            generated::proto::mod_Request::OneOfRequestData::get_block(ref m) => 1 + sizeof_len((m).get_size()),
            generated::proto::mod_Request::OneOfRequestData::get_vertex(ref m) => 1 + sizeof_len((m).get_size()),
            generated::proto::mod_Request::OneOfRequestData::get_preference(ref m) => 1 + sizeof_len((m).get_size()),
            generated::proto::mod_Request::OneOfRequestData::None => 0,
    }    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        match self.RequestData {            generated::proto::mod_Request::OneOfRequestData::get_block(ref m) => { w.write_with_tag(2, |w| w.write_message(m))? },
            generated::proto::mod_Request::OneOfRequestData::get_vertex(ref m) => { w.write_with_tag(10, |w| w.write_message(m))? },
            generated::proto::mod_Request::OneOfRequestData::get_preference(ref m) => { w.write_with_tag(18, |w| w.write_message(m))? },
            generated::proto::mod_Request::OneOfRequestData::None => {},
    }        Ok(())
    }
}

pub mod mod_Request {

use super::*;

#[derive(Debug, PartialEq, Clone)]
pub enum OneOfRequestData {
    get_block(generated::proto::Hash),
    get_vertex(generated::proto::Hash),
    get_preference(generated::proto::Hash),
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
    pub ResponseData: generated::proto::mod_Response::OneOfResponseData,
}

impl<'a> MessageRead<'a> for Response {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(2) => msg.ResponseData = generated::proto::mod_Response::OneOfResponseData::block(r.read_message::<generated::proto::Block>(bytes)?),
                Ok(10) => msg.ResponseData = generated::proto::mod_Response::OneOfResponseData::vertex(r.read_message::<generated::proto::Vertex>(bytes)?),
                Ok(18) => msg.ResponseData = generated::proto::mod_Response::OneOfResponseData::preference(r.read_message::<generated::proto::Preference>(bytes)?),
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
            generated::proto::mod_Response::OneOfResponseData::block(ref m) => 1 + sizeof_len((m).get_size()),
            generated::proto::mod_Response::OneOfResponseData::vertex(ref m) => 1 + sizeof_len((m).get_size()),
            generated::proto::mod_Response::OneOfResponseData::preference(ref m) => 1 + sizeof_len((m).get_size()),
            generated::proto::mod_Response::OneOfResponseData::None => 0,
    }    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        match self.ResponseData {            generated::proto::mod_Response::OneOfResponseData::block(ref m) => { w.write_with_tag(2, |w| w.write_message(m))? },
            generated::proto::mod_Response::OneOfResponseData::vertex(ref m) => { w.write_with_tag(10, |w| w.write_message(m))? },
            generated::proto::mod_Response::OneOfResponseData::preference(ref m) => { w.write_with_tag(18, |w| w.write_message(m))? },
            generated::proto::mod_Response::OneOfResponseData::None => {},
    }        Ok(())
    }
}

pub mod mod_Response {

use super::*;

#[derive(Debug, PartialEq, Clone)]
pub enum OneOfResponseData {
    block(generated::proto::Block),
    vertex(generated::proto::Vertex),
    preference(generated::proto::Preference),
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
pub struct DbRecord {
    pub RequestData: generated::proto::mod_DbRecord::OneOfRequestData,
}

impl<'a> MessageRead<'a> for DbRecord {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(2) => msg.RequestData = generated::proto::mod_DbRecord::OneOfRequestData::vertex(r.read_message::<generated::proto::VertexRec>(bytes)?),
                Ok(10) => msg.RequestData = generated::proto::mod_DbRecord::OneOfRequestData::block_link(r.read_message::<generated::proto::Hash>(bytes)?),
                Ok(18) => msg.RequestData = generated::proto::mod_DbRecord::OneOfRequestData::height_link(r.read_message::<generated::proto::Hashes>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for DbRecord {
    fn get_size(&self) -> usize {
        0
        + match self.RequestData {
            generated::proto::mod_DbRecord::OneOfRequestData::vertex(ref m) => 1 + sizeof_len((m).get_size()),
            generated::proto::mod_DbRecord::OneOfRequestData::block_link(ref m) => 1 + sizeof_len((m).get_size()),
            generated::proto::mod_DbRecord::OneOfRequestData::height_link(ref m) => 1 + sizeof_len((m).get_size()),
            generated::proto::mod_DbRecord::OneOfRequestData::None => 0,
    }    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        match self.RequestData {            generated::proto::mod_DbRecord::OneOfRequestData::vertex(ref m) => { w.write_with_tag(2, |w| w.write_message(m))? },
            generated::proto::mod_DbRecord::OneOfRequestData::block_link(ref m) => { w.write_with_tag(10, |w| w.write_message(m))? },
            generated::proto::mod_DbRecord::OneOfRequestData::height_link(ref m) => { w.write_with_tag(18, |w| w.write_message(m))? },
            generated::proto::mod_DbRecord::OneOfRequestData::None => {},
    }        Ok(())
    }
}

pub mod mod_DbRecord {

use super::*;

#[derive(Debug, PartialEq, Clone)]
pub enum OneOfRequestData {
    vertex(generated::proto::VertexRec),
    block_link(generated::proto::Hash),
    height_link(generated::proto::Hashes),
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
pub struct VertexRec {
    pub height: u64,
    pub vertex: Option<generated::proto::Vertex>,
}

impl<'a> MessageRead<'a> for VertexRec {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(0) => msg.height = r.read_uint64(bytes)?,
                Ok(10) => msg.vertex = Some(r.read_message::<generated::proto::Vertex>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for VertexRec {
    fn get_size(&self) -> usize {
        0
        + if self.height == 0u64 { 0 } else { 1 + sizeof_varint(*(&self.height) as u64) }
        + self.vertex.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.height != 0u64 { w.write_with_tag(0, |w| w.write_uint64(*&self.height))?; }
        if let Some(ref s) = self.vertex { w.write_with_tag(10, |w| w.write_message(s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct PeerInfo {
    pub protocol_version: String,
    pub agent_version: String,
    pub addresses: Vec<String>,
}

impl<'a> MessageRead<'a> for PeerInfo {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(2) => msg.protocol_version = r.read_string(bytes)?.to_owned(),
                Ok(10) => msg.agent_version = r.read_string(bytes)?.to_owned(),
                Ok(18) => msg.addresses.push(r.read_string(bytes)?.to_owned()),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for PeerInfo {
    fn get_size(&self) -> usize {
        0
        + if self.protocol_version == String::default() { 0 } else { 1 + sizeof_len((&self.protocol_version).len()) }
        + if self.agent_version == String::default() { 0 } else { 1 + sizeof_len((&self.agent_version).len()) }
        + self.addresses.iter().map(|s| 1 + sizeof_len((s).len())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.protocol_version != String::default() { w.write_with_tag(2, |w| w.write_string(&**&self.protocol_version))?; }
        if self.agent_version != String::default() { w.write_with_tag(10, |w| w.write_string(&**&self.agent_version))?; }
        for s in &self.addresses { w.write_with_tag(18, |w| w.write_string(&**s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Hash {
    pub hash: Vec<u8>,
}

impl<'a> MessageRead<'a> for Hash {
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

impl MessageWrite for Hash {
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
pub struct Hashes {
    pub hashes: Vec<generated::proto::Hash>,
}

impl<'a> MessageRead<'a> for Hashes {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(2) => msg.hashes.push(r.read_message::<generated::proto::Hash>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Hashes {
    fn get_size(&self) -> usize {
        0
        + self.hashes.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        for s in &self.hashes { w.write_with_tag(2, |w| w.write_message(s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Preference {
    pub hash: Option<generated::proto::Hash>,
    pub preferred: bool,
}

impl<'a> MessageRead<'a> for Preference {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(2) => msg.hash = Some(r.read_message::<generated::proto::Hash>(bytes)?),
                Ok(8) => msg.preferred = r.read_bool(bytes)?,
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Preference {
    fn get_size(&self) -> usize {
        0
        + self.hash.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
        + if self.preferred == false { 0 } else { 1 + sizeof_varint(*(&self.preferred) as u64) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if let Some(ref s) = self.hash { w.write_with_tag(2, |w| w.write_message(s))?; }
        if self.preferred != false { w.write_with_tag(8, |w| w.write_bool(*&self.preferred))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Block {
    pub version: u32,
    pub difficulty: u64,
    pub miner: Vec<u8>,
    pub parents: Vec<generated::proto::Hash>,
    pub inputs: Vec<generated::proto::Hash>,
    pub outputs: Vec<generated::proto::Txo>,
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
                Ok(26) => msg.parents.push(r.read_message::<generated::proto::Hash>(bytes)?),
                Ok(34) => msg.inputs.push(r.read_message::<generated::proto::Hash>(bytes)?),
                Ok(42) => msg.outputs.push(r.read_message::<generated::proto::Txo>(bytes)?),
                Ok(50) => msg.time = r.read_bytes(bytes)?.to_owned(),
                Ok(56) => msg.nonce = r.read_uint64(bytes)?,
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
        + self.parents.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
        + self.inputs.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
        + self.outputs.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
        + if self.time.is_empty() { 0 } else { 1 + sizeof_len((&self.time).len()) }
        + if self.nonce == 0u64 { 0 } else { 1 + sizeof_varint(*(&self.nonce) as u64) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.version != 0u32 { w.write_with_tag(0, |w| w.write_uint32(*&self.version))?; }
        if self.difficulty != 0u64 { w.write_with_tag(8, |w| w.write_uint64(*&self.difficulty))?; }
        if !self.miner.is_empty() { w.write_with_tag(18, |w| w.write_bytes(&**&self.miner))?; }
        for s in &self.parents { w.write_with_tag(26, |w| w.write_message(s))?; }
        for s in &self.inputs { w.write_with_tag(34, |w| w.write_message(s))?; }
        for s in &self.outputs { w.write_with_tag(42, |w| w.write_message(s))?; }
        if !self.time.is_empty() { w.write_with_tag(50, |w| w.write_bytes(&**&self.time))?; }
        if self.nonce != 0u64 { w.write_with_tag(56, |w| w.write_uint64(*&self.nonce))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Vertex {
    pub version: u32,
    pub parents: Vec<generated::proto::Hash>,
    pub block_hash: Option<generated::proto::Hash>,
    pub block: Option<generated::proto::Block>,
}

impl<'a> MessageRead<'a> for Vertex {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(0) => msg.version = r.read_uint32(bytes)?,
                Ok(10) => msg.parents.push(r.read_message::<generated::proto::Hash>(bytes)?),
                Ok(18) => msg.block_hash = Some(r.read_message::<generated::proto::Hash>(bytes)?),
                Ok(26) => msg.block = Some(r.read_message::<generated::proto::Block>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Vertex {
    fn get_size(&self) -> usize {
        0
        + if self.version == 0u32 { 0 } else { 1 + sizeof_varint(*(&self.version) as u64) }
        + self.parents.iter().map(|s| 1 + sizeof_len((s).get_size())).sum::<usize>()
        + self.block_hash.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
        + self.block.as_ref().map_or(0, |m| 1 + sizeof_len((m).get_size()))
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.version != 0u32 { w.write_with_tag(0, |w| w.write_uint32(*&self.version))?; }
        for s in &self.parents { w.write_with_tag(10, |w| w.write_message(s))?; }
        if let Some(ref s) = self.block_hash { w.write_with_tag(18, |w| w.write_message(s))?; }
        if let Some(ref s) = self.block { w.write_with_tag(26, |w| w.write_message(s))?; }
        Ok(())
    }
}

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Txo {
    pub value: u64,
}

impl<'a> MessageRead<'a> for Txo {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(0) => msg.value = r.read_uint64(bytes)?,
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl MessageWrite for Txo {
    fn get_size(&self) -> usize {
        0
        + if self.value == 0u64 { 0 } else { 1 + sizeof_varint(*(&self.value) as u64) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.value != 0u64 { w.write_with_tag(0, |w| w.write_uint64(*&self.value))?; }
        Ok(())
    }
}

