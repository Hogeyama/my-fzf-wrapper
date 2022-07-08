use serde::{de::DeserializeOwned, Deserialize, Serialize};

////////////////////////////////////////////////////////////////////////////////////////////////////
// Method
////////////////////////////////////////////////////////////////////////////////////////////////////

pub trait Method {
    type Param: Serialize + DeserializeOwned;
    type Response: Serialize + DeserializeOwned;
    fn method_name() -> &'static str;
    fn request(self, params: Self::Param) -> Request;
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Request
////////////////////////////////////////////////////////////////////////////////////////////////////

/// Request sent from client to server.
#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum Request {
    Load {
        method: Load,
        params: <Load as Method>::Param,
    },
    Reload {
        method: Reload,
        params: <Reload as Method>::Param,
    },
    Preview {
        method: Preview,
        params: <Preview as Method>::Param,
    },
    Run {
        method: Run,
        params: <Run as Method>::Param,
    },
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Load method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Load;

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct LoadParam {
    pub mode: String,
    pub args: Vec<String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct LoadResp {
    pub header: String,
    pub items: Vec<String>,
}

// pub struct LoadParam {}

impl Method for Load {
    type Param = LoadParam;
    type Response = LoadResp;
    fn method_name() -> &'static str {
        "load"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::Load {
            method: Load,
            params,
        }
    }
}

impl TryFrom<String> for Load {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}
impl Into<String> for Load {
    fn into(self) -> String {
        <Load as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Reload method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Reload;

impl Method for Reload {
    type Param = ();
    type Response = LoadResp;
    fn method_name() -> &'static str {
        "reload"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::Reload {
            method: Reload,
            params,
        }
    }
}

impl TryFrom<String> for Reload {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}
impl Into<String> for Reload {
    fn into(self) -> String {
        <Reload as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Preview method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Preview;

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct PreviewParam {
    pub item: String,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct PreviewResp {
    pub message: String,
}

impl Method for Preview {
    type Param = PreviewParam;
    type Response = PreviewResp;
    fn method_name() -> &'static str {
        "preview"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::Preview {
            method: Preview,
            params,
        }
    }
}

impl TryFrom<String> for Preview {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}
impl Into<String> for Preview {
    fn into(self) -> String {
        <Preview as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Run method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Run;

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct RunParam {
    pub item: String,
    pub args: Vec<String>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct RunResp;

impl Method for Run {
    type Param = RunParam;
    type Response = RunResp;
    fn method_name() -> &'static str {
        "run"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::Run {
            method: Run,
            params,
        }
    }
}

impl TryFrom<String> for Run {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}
impl Into<String> for Run {
    fn into(self) -> String {
        <Run as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Lib
////////////////////////////////////////////////////////////////////////////////////////////////////

fn mk_try_from<T: Method + Default>() -> impl Fn(String) -> Result<T, String> {
    move |s| {
        if s == T::method_name() {
            Ok(Default::default())
        } else {
            Err(s)
        }
    }
}
