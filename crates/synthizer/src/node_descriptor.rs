use std::borrow::Cow;

use crate::channel_format::ChannelFormat;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OutputDescriptor {
    pub(crate) channel_format: ChannelFormat,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NodeDescriptor {
    pub(crate) outputs: Cow<'static, [OutputDescriptor]>,
}
