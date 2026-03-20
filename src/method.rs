use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;

use crate::utils::fzf::PreviewWindow;

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
    Preview {
        method: Preview,
        params: <Preview as Method>::Param,
        preview_window: PreviewWindow,
    },
    Execute {
        method: Execute,
        params: <Execute as Method>::Param,
    },
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Preview method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Preview;

#[derive(Serialize, Deserialize, clap::Parser, Default, Clone, Debug)]
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
        let preview_window = PreviewWindow::from_env().unwrap();
        Request::Preview {
            method: Preview,
            params,
            preview_window,
        }
    }
}

impl PreviewResp {
    pub fn error(err: impl ToString) -> Self {
        Self {
            message: err.to_string(),
        }
    }
}

impl TryFrom<String> for Preview {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}
impl From<Preview> for String {
    fn from(_: Preview) -> Self {
        <Preview as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Execute method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Execute;

impl Method for Execute {
    type Param = ExecuteParam;
    type Response = ();
    fn method_name() -> &'static str {
        "execute_cb"
    }
    fn request(self, params: Self::Param) -> Request {
        Request::Execute {
            method: Execute,
            params,
        }
    }
}

#[derive(Serialize, Deserialize, clap::Parser, Clone, Debug)]
pub struct ExecuteParam {
    pub registered_name: String,
    pub query: String,
    pub item: String,
}

impl TryFrom<String> for Execute {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}

impl From<Execute> for String {
    fn from(_: Execute) -> Self {
        <Execute as Method>::method_name().to_string()
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Load method
////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug)]
#[serde(try_from = "String", into = "String")]
pub struct Load;

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

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct LoadResp {
    pub header: Option<String>,
    pub items: Vec<String>,
    pub is_last: bool,
}

impl LoadResp {
    pub fn new_with_default_header(items: Vec<String>) -> Self {
        let pwd = std::env::current_dir().unwrap().into_os_string();
        Self {
            header: Some(format!("[{}]", pwd.to_string_lossy())),
            items,
            is_last: true,
        }
    }
    pub fn error(err: impl ToString) -> Self {
        Self {
            header: Some("[error]".to_string()),
            items: vec![err.to_string()],
            is_last: true,
        }
    }
    pub fn wip_with_default_header(items: Vec<String>) -> Self {
        let pwd = std::env::current_dir().unwrap().into_os_string();
        Self {
            header: Some(format!("[{}]", pwd.to_string_lossy())),
            items,
            is_last: false,
        }
    }
    pub fn last() -> Self {
        Self {
            header: None,
            items: vec![],
            is_last: true,
        }
    }
}

#[derive(Serialize, Deserialize, clap::Parser, Clone, Debug)]
pub struct LoadParam {
    pub registered_name: String,
    pub query: String,
    pub item: Option<String>,
}

impl TryFrom<String> for Load {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        mk_try_from()(s)
    }
}

impl From<Load> for String {
    fn from(_: Load) -> Self {
        <Load as Method>::method_name().to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_try_from_valid() {
        let result = Load::try_from("load".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn load_try_from_invalid() {
        let result = Load::try_from("preview".to_string());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "preview");
    }

    #[test]
    fn preview_try_from_valid() {
        let result = Preview::try_from("preview".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn preview_try_from_invalid() {
        let result = Preview::try_from("load".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn execute_try_from_valid() {
        let result = Execute::try_from("execute_cb".to_string());
        assert!(result.is_ok());
    }

    #[test]
    fn execute_try_from_invalid() {
        let result = Execute::try_from("execute".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn method_names() {
        assert_eq!(Load::method_name(), "load");
        assert_eq!(Preview::method_name(), "preview");
        assert_eq!(Execute::method_name(), "execute_cb");
    }

    #[test]
    fn into_string_roundtrip() {
        let s: String = Load.into();
        assert_eq!(s, "load");
        let s: String = Preview.into();
        assert_eq!(s, "preview");
        let s: String = Execute.into();
        assert_eq!(s, "execute_cb");
    }

    #[test]
    fn load_resp_error_has_error_header() {
        let resp = LoadResp::error("something went wrong");
        assert_eq!(resp.header.as_deref(), Some("[error]"));
        assert_eq!(resp.items, vec!["something went wrong"]);
        assert!(resp.is_last);
    }

    #[test]
    fn load_resp_last_is_empty() {
        let resp = LoadResp::last();
        assert!(resp.header.is_none());
        assert!(resp.items.is_empty());
        assert!(resp.is_last);
    }

    #[test]
    fn preview_resp_error() {
        let resp = PreviewResp::error("fail");
        assert_eq!(resp.message, "fail");
    }

    #[test]
    fn load_resp_serde_roundtrip() {
        let resp = LoadResp {
            header: Some("[test]".into()),
            items: vec!["a".into(), "b".into()],
            is_last: false,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let resp2: LoadResp = serde_json::from_str(&json).unwrap();
        assert_eq!(resp2.header, resp.header);
        assert_eq!(resp2.items, resp.items);
        assert_eq!(resp2.is_last, resp.is_last);
    }

    #[test]
    fn request_load_serde() {
        let req = Load.request(LoadParam {
            registered_name: "default".into(),
            query: "test".into(),
            item: None,
        });
        let json = serde_json::to_string(&req).unwrap();
        let req2: Request = serde_json::from_str(&json).unwrap();
        assert!(matches!(req2, Request::Load { .. }));
    }
}
