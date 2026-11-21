//! The worker as the conductor starts it: this binary, started again as the child. Sealed, it makes no
//! syscall but its pipes' and its memory's; its heap is capped; it sees no environment.

use std::{
	io::{BufRead, BufReader, Write},
	os::unix::process::ExitStatusExt,
	process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
};

use sa_sandbox::{Answer, Down, Up, WORKER};
use serde_json::json;

fn main() {
	let mut args = std::env::args().skip(1);
	match args.next().as_deref() {
		Some(WORKER) => sa_sandbox::worker(),
		Some("probe") => probe(&args.next().expect("a probe's name")),
		// nextest's protocol: the tests are one, run whole
		Some("--list") =>
			if !std::env::args().any(|a| a == "--ignored") {
				println!("sealed: test");
			},
		_ => {
			let tests: [(&str, fn()); 6] = [
				("sealed, a syscall past its pipes and memory kills it", forbidden_syscalls_kill),
				("a cell's calls go out and their answers come back", a_call_is_answered),
				("the dialect has no load and no while", no_load_no_while),
				("past its heap the session dies, told why", past_the_heap_it_dies),
				("one allocation past the rlimit kills it", past_the_rlimit_it_is_killed),
				("it refuses to run with an environment", it_refuses_an_environment),
			];
			for (name, test) in tests {
				test();
				println!("ok: {name}");
			}
		}
	}
}

/// Sealed, then one syscall it is not let make: never returns.
fn probe(what: &str) {
	sa_sandbox::seal().expect("sealed");
	match what {
		"open" => drop(std::fs::File::open("/etc/hostname")),
		"socket" => drop(std::net::UdpSocket::bind("127.0.0.1:0")),
		"ptrace" => {
			unsafe { libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0) }; // the probe's one syscall
		}
		"nothing" => {}
		other => panic!("no probe {other}"),
	}
	std::process::exit(0);
}

fn child(args: &[&str]) -> Command {
	let mut cmd = Command::new(std::env::current_exe().unwrap());
	cmd.args(args).env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
	cmd
}

fn forbidden_syscalls_kill() {
	assert!(child(&["probe", "nothing"]).status().unwrap().success(), "sealing alone kills nothing");
	for what in ["open", "socket", "ptrace"] {
		let status = child(&["probe", what]).status().unwrap();
		assert_eq!(status.signal(), Some(libc::SIGSYS), "{what}: {status}");
	}
}

struct Worker {
	child: Child,
	stdin: ChildStdin,
	stdout: BufReader<ChildStdout>,
}

impl Worker {
	fn start() -> Self {
		let mut child = child(&[WORKER]).spawn().unwrap();
		Self {
			stdin: child.stdin.take().unwrap(),
			stdout: BufReader::new(child.stdout.take().unwrap()),
			child,
		}
	}

	fn say(&mut self, down: &Down) {
		writeln!(self.stdin, "{}", serde_json::to_string(down).unwrap()).unwrap();
	}

	/// Its next word; `None` once it is gone.
	fn hear(&mut self) -> Option<Up> {
		let mut line = String::new();
		(self.stdout.read_line(&mut line).unwrap() > 0).then(|| serde_json::from_str(&line).unwrap())
	}

	/// What `code` said, to its end.
	fn run(&mut self, code: &str) -> Vec<Up> {
		self.say(&Down::Cell(code.to_owned()));
		let mut said = Vec::new();
		while let Some(up) = self.hear() {
			let end = matches!(up, Up::Done | Up::Threw(_) | Up::Died(_));
			said.push(up);
			if end {
				break;
			}
		}
		said
	}

	fn end(mut self) -> (ExitStatus, String) {
		drop(self.stdin);
		let mut err = String::new();
		std::io::Read::read_to_string(self.child.stderr.as_mut().unwrap(), &mut err).unwrap();
		(self.child.wait().unwrap(), err)
	}
}

fn a_call_is_answered() {
	let mut w = Worker::start();
	w.say(&Down::Cell(
		r#"r = call("get_trade", path = {"id": 7}, body = None)
print(r["status"], r["body"]["name"])"#
			.to_owned(),
	));
	let Some(Up::Call(c)) = w.hear() else { panic!("a call first") };
	assert_eq!(
		(c.operation.as_str(), c.path, c.query, c.body),
		("get_trade", Some(json!({"id": 7}).as_object().unwrap().clone()), None, None)
	);
	w.say(&Down::Answer(Answer {
		status: 200,
		body: json!({ "name": "roofing" }),
	}));
	assert!(matches!(w.hear(), Some(Up::Print(p)) if p == "200 roofing\n"));
	assert!(matches!(w.hear(), Some(Up::Done)));
	let (status, err) = w.end();
	assert!(status.success(), "{status}: {err}");
}

fn no_load_no_while() {
	let mut w = Worker::start();
	for code in ["load(\"x.star\", \"y\")", "while True:\n  pass"] {
		assert!(matches!(w.run(code).as_slice(), [Up::Threw(_)]), "{code}");
	}
	assert!(matches!(w.run("print(json.encode({\"a\": [1]}))").as_slice(), [Up::Print(p), Up::Done] if p == "{\"a\":[1]}\n"));
}

fn past_the_heap_it_dies() {
	let mut w = Worker::start();
	let said = w.run("xs = []\nfor i in range(1000000):\n  xs.append(\"%d\" % i * 50)");
	assert!(matches!(said.as_slice(), [Up::Died(why)] if why.contains("MiB")), "{said:?}");
	let (status, err) = w.end();
	assert_eq!(status.code(), Some(3), "{err}");
}

fn past_the_rlimit_it_is_killed() {
	let mut w = Worker::start();
	let said = w.run("s = \"a\" * (512 * 1024 * 1024)");
	assert!(said.is_empty(), "{said:?}");
	let (status, err) = w.end();
	assert!(status.signal().is_some(), "{status}: {err}");
}

fn it_refuses_an_environment() {
	let out = child(&[WORKER]).env("HOME", "/root").output().unwrap();
	assert_eq!(out.status.code(), Some(2));
	assert!(String::from_utf8_lossy(&out.stderr).contains("environment is not empty"));
}
