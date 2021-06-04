//! ```cargo
//! [dependencies]
//! json = "0.12.4"
//! yaml-rust = "0.4"
//! linked-hash-map = "0.5.3"
//! splinter-proxy = { path = ".." }
//!
//! [dependencies.reqwest]
//! version = "0.11.3"
//! features = ["blocking"]
//! ```

extern crate yaml_rust;

use std::{
    fs::{
        self,
        File,
    },
    io::Write,
    process::{
        Command,
        Stdio,
    },
    str::FromStr,
};

use linked_hash_map::LinkedHashMap;
use splinter_proxy::config::get_config;
use yaml_rust::{
    yaml::{
        self,
        Yaml,
    },
    YamlEmitter,
    YamlLoader,
};

const SERVER_DIR: &'static str = "./servers";
const SERVER_VERSION: &'static str = "1.16.5";

fn main() {
    println!("Loading config.ron");
    let config = get_config("./config.ron");
    let version = SERVER_VERSION;
    // download jars
    let paper_filename = download_project("paper", version);
    let paper_filename = paper_filename.as_str();
    let waterfall_filename = download_project("waterfall", version.rsplit_once('.').unwrap().0);
    let waterfall_filename = waterfall_filename.as_str();

    // set up paper server folders
    for (server_id, server_address) in config.server_addresses.iter() {
        let folder_name = format!("paper_{}", server_id);
        let paper_folder = format!("{}/{}", SERVER_DIR, folder_name.as_str());
        let port = u16::from_str(server_address.split_once(':').unwrap().1).unwrap();
        create_server(paper_filename, paper_folder.as_str());
        println!("Accepting EULA");
        writeln!(
            File::create(format!("{}/eula.txt", paper_folder.as_str())).unwrap(),
            "# Generated by splinter-proxy setup\neula=true"
        );
        println!("Temporarily running paper server to generate files");
        let mut child = Command::new("java")
            .args(&["-jar", paper_filename, "--nogui"])
            .current_dir(paper_folder.as_str())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all("stop\n".as_bytes()).unwrap();
        child.wait().unwrap();
        println!("Modifying server.properties");
        let modified_properties =
            fs::read_to_string(format!("{}/server.properties", paper_folder.as_str()))
                .unwrap()
                .lines()
                .map(|line| {
                    if let Some((left, _)) = line.split_once('=') {
                        match left {
                            "query.port" => return format!("{}={}", left, port),
                            "server-port" => return format!("{}={}", left, port),
                            "online-mode" => return format!("{}=false", left),
                            "spawn-protection" => return format!("{}=0", left),
                            _ => {}
                        }
                    }
                    line.to_string()
                })
                .reduce(|a, b| format!("{}\n{}", a, b))
                .unwrap();
        writeln!(
            File::create(format!("{}/server.properties", paper_folder.as_str())).unwrap(),
            "# Modified by splinter-proxy setup\n{}",
            modified_properties,
        );
    }
    println!("Removing {}", paper_filename);
    fs::remove_file(paper_filename).unwrap();

    // set up waterfall server folders
    let waterfall_folder = format!("{}/waterfall", SERVER_DIR);
    create_server(waterfall_filename, waterfall_folder);
    println!("Removing {}", waterfall_filename);
    fs::remove_file(waterfall_filename).unwrap();
    // set up waterfall yml
    println!("Temporarily running waterfall to generate files");
    let mut child = Command::new("java")
        .args(&["-jar", waterfall_filename])
        .current_dir(waterfall_folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all("end\n".as_bytes()).unwrap();
    child.wait().unwrap();
    println!("Modifying waterfall config.yml");
    let waterfall_config_folder = format!("{}/config.yml", waterfall_folder);
    let mut waterfall_yml =
        YamlLoader::load_from_str(&fs::read_to_string(waterfall_config_folder.as_str()).unwrap())
            .unwrap();
    if let Yaml::Hash(map) = &mut waterfall_yml[0] {
        let mut listeners = map.get(&Yaml::String("listeners".into())).unwrap()[0]
            .clone()
            .into_hash()
            .unwrap();
        listeners.insert(
            Yaml::String("priorities".into()),
            Yaml::Array(vec![Yaml::String("splinter_proxy".into())]),
        );
        map.insert(
            Yaml::String("listeners".into()),
            Yaml::Array(vec![Yaml::Hash(listeners)]),
        );
        let mut servers = map
            .get(&Yaml::String("servers".into()))
            .unwrap()
            .clone()
            .into_hash()
            .unwrap();
        servers.clear();
        let mut server_data = LinkedHashMap::new();
        server_data.insert(
            Yaml::String("motd".into()),
            Yaml::String(config.status.motd.clone()),
        );
        server_data.insert(
            Yaml::String("address".into()),
            Yaml::String(config.bind_address.clone()),
        );
        server_data.insert(Yaml::String("restricted".into()), Yaml::Boolean(false));
        servers.insert(
            Yaml::String("splinter_proxy".into()),
            Yaml::Hash(server_data),
        );
        map.insert(Yaml::String("servers".into()), Yaml::Hash(servers));
        let mut out_str = String::new();
        YamlEmitter::new(&mut out_str).dump(&Yaml::Hash(map.clone()));
        let mut file = File::create(waterfall_config_folder.as_str()).unwrap();
        write!(file, "# Modified by splinter-proxy setup\n{}", out_str);
    }
}

fn create_server(server_filename: &str, target_folder: &str) {
    println!("Creating folder {}", target_folder);
    fs::create_dir_all(target_folder).unwrap();
    let copied_file_path = format!("{}/{}", target_folder, server_filename);
    println!(
        "Copying {} to {}",
        server_filename,
        copied_file_path.as_str()
    );
    fs::copy(server_filename, copied_file_path).unwrap();
}

fn download_project(project_name: &str, project_version: &str) -> String {
    println!(
        "Beginning download process for project {} of version {}",
        project_name, project_version
    );
    println!("Getting build versions from paper api");
    let version_data_text = reqwest::blocking::get(format!(
        "https://papermc.io/api/v2/projects/{}/versions/{}",
        project_name, project_version
    ))
    .unwrap()
    .text()
    .unwrap();
    let parsed_version_data = json::parse(&version_data_text).unwrap();
    let latest_version = parsed_version_data["builds"].members().last().unwrap();
    println!(
        "Got latest build for {}: {}",
        project_version, latest_version
    );
    let filename = format!(
        "{}-{}-{}.jar",
        project_name, project_version, latest_version
    );
    println!("Downloading {}", filename);
    let res = reqwest::blocking::get(format!(
        "https://papermc.io/api/v2/projects/{}/versions/{}/builds/{}/downloads/{}",
        project_name, project_version, latest_version, filename,
    ))
    .unwrap();
    let mut file = File::create(filename.as_str()).unwrap();
    file.write_all(&*res.bytes().unwrap()).unwrap();
    println!("Download for {} complete", filename.as_str());
    filename
}
