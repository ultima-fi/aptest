use clap::{Parser, Subcommand};
use colored::*;

use std::fs::File;
use std::io::{Read, Write};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc::channel;
use std::thread::sleep;
use std::time::Duration;
use yaml_rust::YamlLoader;

macro_rules! pretty_expect {
    ($e:expr, $msg:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => {
                println!("\n{}\n", $msg);
                println!("{}\n", e);
                std::process::exit(1);
            }
        }
    };
}
macro_rules! cleanup_expect {
    ($e:expr, $msg:expr, $children:expr, $args:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => {
                println!("\n{}\n", $msg);
                println!("{}\n", e);
                cleanup($children, $args);
                std::process::exit(1);
            }
        }
    };
}
macro_rules! make_file {
    ($path:expr, $content:expr) => {
        let mut file = pretty_expect!(File::create($path), "Failed to create file");
        pretty_expect!(
            file.write_all($content.as_bytes()),
            "Failed to write to file"
        );
    };
}
macro_rules! make_dir {
    ($path:expr) => {
        pretty_expect!(
            std::fs::create_dir_all($path),
            format!("Could not create directory {}", $path)
        );
    };
}

///A small framework to assist in testing aptos programs
#[derive(Parser)]
#[clap(version, about, long_about = None)]
struct Sub {
    #[clap(subcommand)]
    cmd: Subcmds,
}
#[derive(Parser)]
struct Args {
    ///Removes call to "aptos move compile"
    #[clap(short = 'c', long)]
    no_compile: bool,

    ///Removes call to "aptos move publish"
    #[clap(short = 'p', long)]
    no_publish: bool,

    ///Specifies the number of seconds to wait on the validator
    ///spinning up before trying to interact with it
    #[clap(short = 'd', long, default_value = "14")]
    start_delay: u64,

    ///Run just the validator node, without a faucet
    #[clap(long, short = 'f')]
    no_faucet: bool,

    ///Starts validator and waits for Ctrl+C so that end to end tests can be run manually
    #[clap(long, short)]
    interactive: bool,

    ///Logs the output of the validator to a file
    #[clap(long = "log", short)]
    log_node: bool,
}

#[derive(Subcommand)]
enum Subcmds {
    ///Initialize a new project
    Init { name: String },

    ///Runs the framework in the current directory
    Run(Args),
}

fn main() {
    let sub = Sub::parse();

    //If the sub command is init, call the init function,
    //else return runargs 
    let args = match sub.cmd {
        Subcmds::Init { name } => init(name),
        Subcmds::Run(runargs) => runargs,
    };

    let (tx, rx) = channel();

    ctrlc::set_handler(move || {
        tx.send(())
            .expect("Could not send signal to setup Ctrl-C handler")
    })
    .expect("Could not set Ctrl-C handler");

    //Compilation
    if !args.no_compile {
        println!("\n{}\n", "Compiling Move code...".bright_blue().bold());
        let exit_code = Command::new("aptos")
            .args(["move", "compile"])
            .status()
            .expect("Couldn't find aptos command. Is it installed ?");
        if !exit_code.success() {
            println!(
                "\n{}\n",
                "Compilation failed, exiting early...".bright_red().bold()
            );
            //Cleanup not needed because nodes haven't been started yet
            std::process::exit(1);
        }
    }

    //Local Node start
    let children = start_node(&args);

    if !args.no_publish {
        match publish() {
            Ok(_) => {
                println!("\n{}\n", "Deployment successful.".bright_green().bold());
            }
            Err(err) => {
                println!(
                    "\n{}{}\n",
                    "Error: ".bright_red().bold(),
                    err.bright_red().bold()
                );
                cleanup(children, &args);
                std::process::exit(1);
            }
        }
    }

    if args.interactive {
        println!("\n{}\n", "Local Node is running.".bright_green().bold());
        println!(
            "{}\n",
            "End to End tests can be run separately now, or Ctrl+C\nto exit tool and close node..."
                .bright_blue()
                .bold()
        );
        rx.recv().expect("Could not receive from channel.");
    } else {
        //Start End to End tests and wait for them to finish
        let mut e2e_child = cleanup_expect!(
            e2e_tests(),
            "Error running e2e tests".bright_red().bold(),
            children,
            &args
        );
        e2e_child.wait().expect("Could not wait on npm child");
    }

    cleanup(children, &args);
    println!("\n{}", "Done".bright_green().bold());
}

//Cleans up running nodes and logs them if requested
fn cleanup(children: (Child, Option<Child>, String), args: &Args) {
    let mut node_child = children.0;
    let maybe_faucet_child = children.1;
    let scanned_output = children.2;
    //Close node and faucet
    println!("\n{}\n", "Closing local node...".bright_blue().bold());
    node_child
        .kill()
        .expect("Could not kill validator process.");
    let node_output = node_child
        .wait_with_output()
        .expect("Could not wait on validator.");
    let node_output = String::from_utf8_lossy(&node_output.stdout[..]).to_string();

    let foutput: Option<Output>;
    let mut faucet_output = String::new();
    if let Some(mut faucet_child) = maybe_faucet_child {
        faucet_child.kill().expect("Could not kill faucet process.");
        foutput = Some(
            faucet_child
                .wait_with_output()
                .expect("Could not wait on faucet."),
        );
        faucet_output = String::from_utf8_lossy(&foutput.unwrap().stderr[..]).to_string();
    }

    //Write out node's log if requested
    if args.log_node {
        let mut log_file = File::create("validator.log").expect("Could not create log file.");
        let mut log_string = scanned_output;
        log_string.push_str(node_output.as_str());
        log_string.push_str(faucet_output.as_str());
        log_file
            .write_all(log_string.as_bytes())
            .expect("Could not write to log file.");
    }
}

///Start the local node and return a tuple of the child process and
/// optional faucet child process
fn start_node(args: &Args) -> (Child, Option<Child>, String) {
    println!(
        "\n{}\n",
        "Starting local validator node...".bright_blue().bold()
    );

    let node_attempt = Command::new("aptos-node")
        .args(["--test"])
        .stdout(Stdio::piped())
        .spawn();

    let mut node_child = pretty_expect!(
        node_attempt,
        "Could not find the aptos-node command. Is it installed ?..."
            .bright_red()
            .bold()
    );

    //This is hardcoded because since the validator runs constantly
    //it doesn't print EOF in the stdout stream, so we have to grab
    //a predetermined amount of bytes. 450 bytes should be enough
    //to find the mint key file, but there is likely a more robust
    //way to do this.
    let mut buffer: [u8; 450] = [0; 450];
    node_child
        .stdout
        .as_mut()
        .expect("Could not get stdout reference from node child process")
        .read_exact(&mut buffer)
        .expect("Could not read from node child process stdout");

    let node_output = String::from_utf8_lossy(&buffer[..]).to_string();

    let mint_key_path = find_mint_path(node_output.clone());

    if !args.no_faucet {
        sleep(Duration::from_secs(args.start_delay / 2));
        let faucet_attempt = Command::new("aptos-faucet")
            .args([
                "--chain-id",
                "TESTING",
                "--mint-key-file-path",
                mint_key_path.as_str(),
                "--address",
                "0.0.0.0",
                "--port",
                "8000",
                "--server-url",
                "http://localhost:8080",
            ])
            .stderr(Stdio::piped())
            .spawn();

        let faucet_child = cleanup_expect!(
            faucet_attempt,
            "Could not find the aptos-faucet command. Is it installed ?..."
                .bright_red()
                .bold(),
            (node_child, None, node_output),
            args
        );

        sleep(Duration::from_secs(args.start_delay / 2));
        return (node_child, Some(faucet_child), node_output);
    }
    sleep(Duration::from_secs(args.start_delay));

    (node_child, None, node_output)
}

/// Publish the contract to the validator node,
/// will halt and error if the publishing fails
fn publish() -> Result<(), String> {
    //-----------------------------Funding--------------------------------------
    println!(
        "\n{}\n",
        "Funding new account on local node...".bright_blue().bold()
    );

    let account = fetch_account();
    let account = account.as_str();

    Command::new("aptos")
        .args([
            "account",
            "fund",
            "--faucet-url",
            "http://0.0.0.0:8000",
            "--account",
            account,
        ])
        .status()
        .expect("Couldn't find aptos command. Is it installed ?");

    //-----------------------------Deploying-------------------------------------
    println!("\n{}\n", "Deploying move code...".bright_blue().bold());
    let publish_code = Command::new("aptos")
        .args(["move", "publish", "--url", "http://0.0.0.0:8080"])
        .status()
        .expect("Couldn't find aptos command. Is it installed ?");

    //------------------------Error Handling of Publish--------------------------
    if !publish_code.success() {
        Err("Aptos reports publish failed".to_string())
    } else {
        Ok(())
    }
}

//Runs the tests with "npm run test"
fn e2e_tests() -> Result<Child, std::io::Error> {
    println!("\n{}\n", "Running e2e tests...".bright_blue().bold());
    Command::new("npm").args(["run", "test"]).spawn()
}

//------------------------------------------------------------------------------
//                             Helper Functions
//------------------------------------------------------------------------------

/// Fetch the account from the aptos config file
/// for funding it on the local node.
fn fetch_account() -> String {
    let config_file = std::fs::read_to_string(".aptos/config.yaml")
        .expect("Couldn't find .aptos/config.yaml. Did you run aptos init?");
    let config_yaml =
        YamlLoader::load_from_str(&config_file).expect("Could not parse aptos config file");
    let config_yaml = &config_yaml[0];
    let account = &config_yaml["profiles"]["default"]["account"]
        .as_str()
        .expect("Could not find a default account in config file");
    account.to_string()
}

/// Finds the path to the mint key file in the node's output.
fn find_mint_path(line: String) -> String {
    let mut path =
    line.split(':')
        .skip_while(|x| !x.contains("Aptos root key path"))
        .nth(1)
        .expect("Could not find Aptos root key path in line. Perhaps give the node more time to spin up?")
        .split('\n')
        .next()
        .unwrap()
        .trim()
        .to_string();
    path.retain(|x| x != '\"');
    path
}

#[test]
fn test_mint_path() {
    let mint_path = find_mint_path(
        "Aptos root key path: \"/home/user/.aptos/mint.key\"\nWaypoint: stuff".to_string(),
    );
    dbg!(&mint_path);
    assert_eq!(mint_path, "/home/user/.aptos/mint.key");
}

//Init all the files and directories for a new project if they don't exist.
//Should never return to main.
fn init(name: String) -> ! {
    //check for Move.toml
    if std::fs::read_to_string("./Move.toml").is_ok() {
        println!(
            "\n{}\n",
            "Move.toml file already exists here!".bright_blue().bold()
        );
        std::process::exit(1);
    }

    //run aptos move init --name args.init.name
    let init_attempt = Command::new("aptos")
        .args(["move", "init", "--name", name.as_str()])
        .spawn();

    let mut init_child = pretty_expect!(
        init_attempt,
        "Couldn't find aptos command. Is it installed ?"
            .bold()
            .bright_blue()
    );

    pretty_expect!(
        init_child.wait(),
        "Could not wait for aptos move init to finish"
    );

    let package_json = format!(
"{{
    \"name\": \"test_{}\",
    \"version\": \"1.0.0\",
    \"scripts\": {{
      \"test\": \"env TS_NODE_COMPILER_OPTIONS='{{\\\"module\\\": \\\"commonjs\\\" }}' mocha -r ts-node/register 'tests/**/*.ts'\"
    }},
    \"dependencies\": {{
      \"@types/chai\": \"^4.3.1\",
      \"@types/mocha\": \"^9.1.1\",
      \"aptos\": \"^1.2.0\",
      \"chai\": \"^4.3.6\",
      \"mocha\": \"^10.0.0\",
      \"ts-mocha\": \"^10.0.0\",
      \"typescript\": \"^4.7.4\",
    }}
}}",
    name.as_str()
    );

    make_file!("./package.json", package_json);
    make_dir!("./tests");

    let install_attempt = Command::new("npm").args(["install"]).spawn();

    println!("\n{}\n", "Installing dependencies...".bright_blue().bold());
    let mut install_child = pretty_expect!(
        install_attempt,
        "Couldn't find npm command. Is it installed ?"
            .bold()
            .bright_blue()
    );
    pretty_expect!(
        install_child.wait(),
        "Could not wait for npm install to finish"
    );
    std::process::exit(0);
}
