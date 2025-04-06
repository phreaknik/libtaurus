// Automatically generated rust module for 'rpc_messages.proto' file

#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(unused_imports)]
#![allow(unknown_lints)]
#![allow(clippy::all)]
#![cfg_attr(rustfmt, rustfmt_skip)]


use std::borrow::Cow;
use quick_protobuf::{MessageInfo, MessageRead, MessageWrite, BytesReader, Writer, WriterBackend, Result};
use core::convert::{TryFrom, TryInto};
use quick_protobuf::sizeofs::*;
use super::*;

#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Request<'a> {
    pub payload: rpc_messages::mod_Request::OneOfpayload<'a>,
}

impl<'a> MessageRead<'a> for Request<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(810) => msg.payload = rpc_messages::mod_Request::OneOfpayload::get_block(r.read_message::<rpc_messages::GetBlock>(bytes)?),
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for Request<'a> {
    fn get_size(&self) -> usize {
        0
        + match self.payload {
            rpc_messages::mod_Request::OneOfpayload::get_block(ref m) => 2 + sizeof_len((m).get_size()),
            rpc_messages::mod_Request::OneOfpayload::None => 0,
    }    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        match self.payload {            rpc_messages::mod_Request::OneOfpayload::get_block(ref m) => { w.write_with_tag(810, |w| w.write_message(m))? },
            rpc_messages::mod_Request::OneOfpayload::None => {},
    }        Ok(())
    }
}


            // IMPORTANT: For any future changes, note that the lifetime parameter
            // of the `proto` field is set to 'static!!!
            //
            // This means that the internals of `proto` should at no point create a
            // mutable reference to something using that lifetime parameter, on pain
            // of UB. This applies even though it may be transmuted to a smaller
            // lifetime later (through `proto()` or `proto_mut()`).
            //
            // At the time of writing, the only possible thing that uses the
            // lifetime parameter is `Cow<'a, T>`, which never does this, so it's
            // not UB.
            //
            #[derive(Debug)]
            struct RequestOwnedInner {
                buf: Vec<u8>,
                proto: Option<Request<'static>>,
                _pin: core::marker::PhantomPinned,
            }

            impl RequestOwnedInner {
                fn new(buf: Vec<u8>) -> Result<core::pin::Pin<Box<Self>>> {
                    let inner = Self {
                        buf,
                        proto: None,
                        _pin: core::marker::PhantomPinned,
                    };
                    let mut pinned = Box::pin(inner);

                    let mut reader = BytesReader::from_bytes(&pinned.buf);
                    let proto = Request::from_reader(&mut reader, &pinned.buf)?;

                    unsafe {
                        let proto = core::mem::transmute::<_, Request<'_>>(proto);
                        pinned.as_mut().get_unchecked_mut().proto = Some(proto);
                    }
                    Ok(pinned)
                }
            }

            pub struct RequestOwned {
                inner: core::pin::Pin<Box<RequestOwnedInner>>,
            }

            #[allow(dead_code)]
            impl RequestOwned {
                pub fn buf(&self) -> &[u8] {
                    &self.inner.buf
                }

                pub fn proto<'a>(&'a self) -> &'a Request<'a> {
                    let proto = self.inner.proto.as_ref().unwrap();
                    unsafe { core::mem::transmute::<&Request<'static>, &Request<'a>>(proto) }
                }

                pub fn proto_mut<'a>(&'a mut self) -> &'a mut Request<'a> {
                    let inner = self.inner.as_mut();
                    let inner = unsafe { inner.get_unchecked_mut() };
                    let proto = inner.proto.as_mut().unwrap();
                    unsafe { core::mem::transmute::<_, &mut Request<'a>>(proto) }
                }
            }

            impl core::fmt::Debug for RequestOwned {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    self.inner.proto.as_ref().unwrap().fmt(f)
                }
            }

            impl TryFrom<Vec<u8>> for RequestOwned {
                type Error=quick_protobuf::Error;

                fn try_from(buf: Vec<u8>) -> Result<Self> {
                    Ok(Self { inner: RequestOwnedInner::new(buf)? })
                }
            }

            impl TryInto<Vec<u8>> for RequestOwned {
                type Error=quick_protobuf::Error;

                fn try_into(self) -> Result<Vec<u8>> {
                    let mut buf = Vec::new();
                    let mut writer = Writer::new(&mut buf);
                    self.inner.proto.as_ref().unwrap().write_message(&mut writer)?;
                    Ok(buf)
                }
            }

            impl From<Request<'static>> for RequestOwned {
                fn from(proto: Request<'static>) -> Self {
                    Self {
                        inner: Box::pin(RequestOwnedInner {
                            buf: Vec::new(),
                            proto: Some(proto),
                            _pin: core::marker::PhantomPinned,
                        })
                    }
                }
            }
            
pub mod mod_Request {

use super::*;

#[derive(Debug, PartialEq, Clone)]
pub enum OneOfpayload<'a> {
    get_block(rpc_messages::GetBlock<'a>),
    None,
}

impl<'a> Default for OneOfpayload<'a> {
    fn default() -> Self {
        OneOfpayload::None
    }
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
            
#[allow(clippy::derive_partial_eq_without_eq)]
#[derive(Debug, Default, PartialEq, Clone)]
pub struct GetBlock<'a> {
    pub height: u64,
    pub hash: Cow<'a, [u8]>,
}

impl<'a> MessageRead<'a> for GetBlock<'a> {
    fn from_reader(r: &mut BytesReader, bytes: &'a [u8]) -> Result<Self> {
        let mut msg = Self::default();
        while !r.is_eof() {
            match r.next_tag(bytes) {
                Ok(2408) => msg.height = r.read_uint64(bytes)?,
                Ok(2418) => msg.hash = r.read_bytes(bytes).map(Cow::Borrowed)?,
                Ok(t) => { r.read_unknown(bytes, t)?; }
                Err(e) => return Err(e),
            }
        }
        Ok(msg)
    }
}

impl<'a> MessageWrite for GetBlock<'a> {
    fn get_size(&self) -> usize {
        0
        + if self.height == 0u64 { 0 } else { 2 + sizeof_varint(*(&self.height) as u64) }
        + if self.hash == Cow::Borrowed(b"") { 0 } else { 2 + sizeof_len((&self.hash).len()) }
    }

    fn write_message<W: WriterBackend>(&self, w: &mut Writer<W>) -> Result<()> {
        if self.height != 0u64 { w.write_with_tag(2408, |w| w.write_uint64(*&self.height))?; }
        if self.hash != Cow::Borrowed(b"") { w.write_with_tag(2418, |w| w.write_bytes(&**&self.hash))?; }
        Ok(())
    }
}


            // IMPORTANT: For any future changes, note that the lifetime parameter
            // of the `proto` field is set to 'static!!!
            //
            // This means that the internals of `proto` should at no point create a
            // mutable reference to something using that lifetime parameter, on pain
            // of UB. This applies even though it may be transmuted to a smaller
            // lifetime later (through `proto()` or `proto_mut()`).
            //
            // At the time of writing, the only possible thing that uses the
            // lifetime parameter is `Cow<'a, T>`, which never does this, so it's
            // not UB.
            //
            #[derive(Debug)]
            struct GetBlockOwnedInner {
                buf: Vec<u8>,
                proto: Option<GetBlock<'static>>,
                _pin: core::marker::PhantomPinned,
            }

            impl GetBlockOwnedInner {
                fn new(buf: Vec<u8>) -> Result<core::pin::Pin<Box<Self>>> {
                    let inner = Self {
                        buf,
                        proto: None,
                        _pin: core::marker::PhantomPinned,
                    };
                    let mut pinned = Box::pin(inner);

                    let mut reader = BytesReader::from_bytes(&pinned.buf);
                    let proto = GetBlock::from_reader(&mut reader, &pinned.buf)?;

                    unsafe {
                        let proto = core::mem::transmute::<_, GetBlock<'_>>(proto);
                        pinned.as_mut().get_unchecked_mut().proto = Some(proto);
                    }
                    Ok(pinned)
                }
            }

            pub struct GetBlockOwned {
                inner: core::pin::Pin<Box<GetBlockOwnedInner>>,
            }

            #[allow(dead_code)]
            impl GetBlockOwned {
                pub fn buf(&self) -> &[u8] {
                    &self.inner.buf
                }

                pub fn proto<'a>(&'a self) -> &'a GetBlock<'a> {
                    let proto = self.inner.proto.as_ref().unwrap();
                    unsafe { core::mem::transmute::<&GetBlock<'static>, &GetBlock<'a>>(proto) }
                }

                pub fn proto_mut<'a>(&'a mut self) -> &'a mut GetBlock<'a> {
                    let inner = self.inner.as_mut();
                    let inner = unsafe { inner.get_unchecked_mut() };
                    let proto = inner.proto.as_mut().unwrap();
                    unsafe { core::mem::transmute::<_, &mut GetBlock<'a>>(proto) }
                }
            }

            impl core::fmt::Debug for GetBlockOwned {
                fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                    self.inner.proto.as_ref().unwrap().fmt(f)
                }
            }

            impl TryFrom<Vec<u8>> for GetBlockOwned {
                type Error=quick_protobuf::Error;

                fn try_from(buf: Vec<u8>) -> Result<Self> {
                    Ok(Self { inner: GetBlockOwnedInner::new(buf)? })
                }
            }

            impl TryInto<Vec<u8>> for GetBlockOwned {
                type Error=quick_protobuf::Error;

                fn try_into(self) -> Result<Vec<u8>> {
                    let mut buf = Vec::new();
                    let mut writer = Writer::new(&mut buf);
                    self.inner.proto.as_ref().unwrap().write_message(&mut writer)?;
                    Ok(buf)
                }
            }

            impl From<GetBlock<'static>> for GetBlockOwned {
                fn from(proto: GetBlock<'static>) -> Self {
                    Self {
                        inner: Box::pin(GetBlockOwnedInner {
                            buf: Vec::new(),
                            proto: Some(proto),
                            _pin: core::marker::PhantomPinned,
                        })
                    }
                }
            }
            
