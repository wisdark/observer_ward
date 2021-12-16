#[macro_use]
extern crate lazy_static;

use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::io::Read;
use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::{io, net::SocketAddr, time::Duration};

use futures::stream::FuturesUnordered;
use futures::StreamExt;
use regex::bytes::Regex;
use serde::{Deserialize, Serialize};

use unescape_lib::unescape_func;

mod unescape_lib;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Matches {
    service: String,
    #[serde(deserialize_with = "unescape_func")]
    pattern: Vec<u8>,
    version_info: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct NmapFingerPrintLib {
    matches: Vec<Matches>,
    directive_name: String,
    protocol: String,
    #[serde(deserialize_with = "unescape_func")]
    directive_str: Vec<u8>,
    total_wait_ms: Option<u8>,
    tcp_wrapped_ms: Option<u8>,
    #[serde(default)]
    rarity: u8,
    #[serde(default)]
    ports: Vec<u16>,
    ssl_ports: Option<String>,
    fallback: Option<String>,
}

impl NmapFingerPrintLib {
    pub async fn match_rules(&self, response: &Vec<u8>) -> Vec<String> {
        let mut server_name: Vec<String> = Vec::new();
        let mut futures = FuturesUnordered::new();
        let mut matches_iter = self.matches.iter();
        for _ in 0..100 {
            if let Some(rule) = matches_iter.next() {
                futures.push(self.what_server(&rule, response));
            } else {
                break;
            }
        }
        while let Some(result) = futures.next().await {
            if let Some(rule) = matches_iter.next() {
                futures.push(self.what_server(&rule, response));
            }
            if !result.is_empty() {
                server_name.push(result);
            }
        }
        if !server_name.is_empty() {
            println!("{:?}", server_name);
        }
        server_name
    }
    async fn what_server(&self, rule: &Matches, text: &Vec<u8>) -> String {
        let regex_str = std::str::from_utf8(&rule.pattern);
        if let Ok(ok_regex_str) = regex_str {
            return match Regex::new(&ok_regex_str) {
                Ok(re) => {
                    if re.captures(text).is_some() {
                        rule.service.clone()
                    } else {
                        String::new()
                    }
                }
                Err(_) => String::new(),
            };
        }
        String::new()
    }
}

lazy_static! {
    static ref NMAP_FINGERPRINT_LIB_DATA: Vec<NmapFingerPrintLib> = {
        let self_path: PathBuf = env::current_exe().unwrap_or(PathBuf::new());
        let path = Path::new(&self_path).parent().unwrap_or(Path::new(""));
        let mut file = match File::open(path.join("nmap-service-probes.json")) {
            Err(_) => {
                println!("The nmap fingerprint library cannot be found in the current directory!");
                std::process::exit(0);
            }
            Ok(file) => file,
        };
        let mut data = String::new();
        file.read_to_string(&mut data).unwrap();
        let nmap_fingerprint: Vec<NmapFingerPrintLib> =
            serde_json::from_str(&data).expect("BAD JSON");
        nmap_fingerprint
    };
}

#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct WhatServer {
    timeout: u64,
}

impl WhatServer {
    pub fn new(timeout: u64) -> Self {
        Self { timeout }
    }
    fn filter_probes_by_port(
        &self,
        port: u16,
    ) -> (Vec<NmapFingerPrintLib>, Vec<NmapFingerPrintLib>) {
        let (mut in_probes, mut ex_probes): (Vec<NmapFingerPrintLib>, Vec<NmapFingerPrintLib>) =
            (vec![], vec![]);
        for nmap_fingerprint in NMAP_FINGERPRINT_LIB_DATA.clone().into_iter() {
            if nmap_fingerprint.ports.contains(&port) {
                in_probes.push(nmap_fingerprint);
            } else {
                ex_probes.push(nmap_fingerprint);
            }
        }
        return (in_probes, ex_probes);
    }
    fn send_directive_str_request(&self, socket: SocketAddr, payload: Vec<u8>) -> Vec<u8> {
        let received: Vec<u8> = Vec::new();
        if let Ok(mut stream) = self.connect(socket) {
            stream
                .set_write_timeout(Some(Duration::from_millis(self.timeout)))
                .unwrap_or_default();
            stream
                .set_read_timeout(Some(Duration::from_millis(self.timeout)))
                .unwrap_or_default();
            stream.write_all(&payload).unwrap();
            stream.flush().unwrap();
            let mut reader = io::BufReader::new(&mut stream);
            let received: Vec<u8> = reader.fill_buf().unwrap_or_default().to_vec();
            reader.consume(received.len());
            return received;
        };
        received
    }

    fn connect(&self, socket: SocketAddr) -> io::Result<TcpStream> {
        let stream = TcpStream::connect(socket).unwrap();
        stream.set_nodelay(true).unwrap();
        stream.set_ttl(100).unwrap();
        Ok(stream)
    }
    async fn exec_run(&self, probe: NmapFingerPrintLib, host_port: SocketAddr) -> Vec<String> {
        let response = self.send_directive_str_request(host_port, probe.directive_str.clone());
        let server = probe.match_rules(&response).await;
        return server;
    }
    pub async fn scan(&self, host_port: SocketAddr) -> HashSet<String> {
        let (in_probes, ex_probes) = self.filter_probes_by_port(host_port.port());
        let mut in_probes_iter = in_probes.into_iter();
        let mut ex_probes_iter = ex_probes.into_iter();
        let mut futures = FuturesUnordered::new();
        let mut server_set: HashSet<String> = HashSet::new();
        for _ in 0..16 {
            if let Some(socket) = in_probes_iter.next() {
                futures.push(self.exec_run(socket, host_port));
            } else {
                break;
            }
        }
        while let Some(result) = futures.next().await {
            if let Some(probes) = in_probes_iter.next() {
                futures.push(self.exec_run(probes, host_port));
            }
            if !result.is_empty() {
                server_set.extend(result);
            }
        }
        if !server_set.is_empty() {
            return server_set;
        }
        let mut futures = FuturesUnordered::new();
        for _ in 0..16 {
            if let Some(probes) = ex_probes_iter.next() {
                futures.push(self.exec_run(probes, host_port));
            }
        }
        while let Some(result) = futures.next().await {
            if let Some(socket) = ex_probes_iter.next() {
                futures.push(self.exec_run(socket, host_port));
            }
            if !result.is_empty() {
                server_set.extend(result);
            }
        }
        return server_set;
    }
}

// use std::net::{IpAddr, SocketAddr};
// use what_server::WhatServer;
//
// #[tokio::main]
// async fn main() {
//     let ip = "127.0.0.1".parse::<IpAddr>().unwrap();
//     let socket = SocketAddr::new(ip, 22);
//     let what_server = WhatServer::new(300);
//     let server = what_server.scan(socket).await;
//     println!("{:?}", server);
// }