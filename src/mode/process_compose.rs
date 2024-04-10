use crate::{
    bindings,
    config::Config,
    method::{LoadResp, PreviewResp},
    mode::{config_builder, ModeDef},
    state::State,
    utils::fzf,
};

use futures::{future::BoxFuture, FutureExt};

use super::CallbackMap;

#[derive(Clone)]
pub struct ProcessCompose;

impl ProcessCompose {
    pub fn new() -> Self {
        Self
    }
}

struct Item {
    process: String,
}

impl Item {
    fn parse(item: String) -> Self {
        Self { process: item }
    }
}

mod dto {
    #[derive(serde::Deserialize, Debug)]
    pub(crate) struct Processes {
        pub(crate) data: Vec<Process>,
    }
    #[derive(serde::Deserialize, Debug)]
    pub(crate) struct Process {
        pub(crate) name: String,
    }
    #[derive(serde::Deserialize, Debug)]
    pub struct Logs {
        pub(crate) logs: Vec<String>,
    }
}

impl ModeDef for ProcessCompose {
    fn name(&self) -> &'static str {
        "process-compose"
    }
    fn load<'a>(
        &'a mut self,
        _config: &'a Config,
        _state: &'a mut State,
        _query: String,
        _item: String,
    ) -> BoxFuture<'a, Result<LoadResp, String>> {
        async move {
            let host = get_host()?;
            let processes = reqwest::get(format!("{host}/processes"))
                .await
                .map_err(|e| e.to_string())?
                .json::<dto::Processes>()
                .await
                .map_err(|e| e.to_string())?;
            let mut items = processes
                .data
                .into_iter()
                .map(|p| p.name)
                .collect::<Vec<_>>();
            items.sort();
            Ok(LoadResp::new_with_default_header(items))
        }
        .boxed()
    }
    fn preview(
        &self,
        _config: &Config,
        _state: &mut State,
        item: String,
    ) -> BoxFuture<'static, Result<PreviewResp, String>> {
        async move {
            let Item { process } = Item::parse(item);
            let host = get_host()?;
            // NOTE: なぜかFZF_PREVIEW_LINESが渡ってこない
            let height = std::env::var("FZF_PREVIEW_LINES")
                .unwrap_or("50".to_string())
                .parse::<usize>()
                .unwrap_or(50);
            let limit = 0; // 0 will get all the lines till the end
            let logs = reqwest::get(format!("{host}/process/logs/{process}/{height}/{limit}"))
                .await
                .map_err(|e| e.to_string())?
                .json::<dto::Logs>()
                .await
                .map_err(|e| e.to_string())?;
            let message = logs.logs.join("\n");
            Ok(PreviewResp { message })
        }
        .boxed()
    }
    fn fzf_bindings(&self) -> (fzf::Bindings, CallbackMap) {
        use config_builder::*;
        bindings! {
            b <= default_bindings(),
            "enter" => [
                select_and_execute!{b, |_mode,_config,_state,_query,item|
                    "restart" => {
                        restart(Item::parse(item)).await?;
                        Ok(())
                    },
                    "start" => {
                        start(Item::parse(item)).await?;
                        Ok(())
                    },
                    "stop" => {
                        stop(Item::parse(item)).await?;
                        Ok(())
                    },
                },
                b.reload(),
            ],
            "ctrl-e" => [
                execute_silent!(b, |_mode,_config,_state,_query,item| {
                    restart(Item::parse(item)).await?;
                    Ok(())
                }),
                b.reload()
            ],
            "right" => [
                b.reload()
            ],
        }
    }
}

fn get_host() -> Result<String, String> {
    std::env::var("FZFW_PROCESS_COMPOSE_HOST").map_err(|_| "No host".to_string())
}

async fn restart(item: Item) -> Result<(), String> {
    let Item { process } = item;
    let host = get_host()?;
    let client = reqwest::Client::new();
    let _processes = client
        .post(format!("{host}/process/restart/{process}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn start(item: Item) -> Result<(), String> {
    let Item { process } = item;
    let host = get_host()?;
    let client = reqwest::Client::new();
    let _processes = client
        .post(format!("{host}/process/start/{process}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn stop(item: Item) -> Result<(), String> {
    let Item { process } = item;
    let host = get_host()?;
    let client = reqwest::Client::new();
    let _processes = client
        .post(format!("{host}/process/stop/{process}"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
