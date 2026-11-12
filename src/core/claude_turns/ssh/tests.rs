use super::ssh_hosts;

fn hosts(command: &str) -> Vec<String> {
    ssh_hosts(command)
}

#[test]
fn every_form_seen_in_real_transcripts_finds_the_host() {
    let cases = [
        ("ssh rdehome2 'uptime'", "rdehome2"),
        ("ssh -o BatchMode=yes rdeserverpc 'df -h'", "rdeserverpc"),
        (
            "/c/Windows/System32/OpenSSH/ssh.exe rdeserverpc ls",
            "rdeserverpc",
        ),
        (
            "C:/Windows/System32/OpenSSH/ssh.exe rdehome2 sh -s < probe",
            "rdehome2",
        ),
        ("wsl.exe -e bash -lc 'ssh rdehome2 uptime'", "rdehome2"),
        ("timeout 20 ssh -F x -o A=b host uptime", "host"),
        ("cd /tmp && ssh -tt rde@rdehome1 ls", "rdehome1"),
        (
            "\"C:\\Windows\\System32\\OpenSSH\\ssh.exe\" -p 22 rdeoffice1",
            "rdeoffice1",
        ),
        ("env A=1 ssh -J jump rdehome2", "rdehome2"),
    ];
    for (command, host) in cases {
        assert_eq!(hosts(command), vec![host.to_string()], "{command}");
    }
}

#[test]
fn ssh_as_a_mere_word_does_not_count() {
    for command in [
        "echo ssh rdehome2",
        "grep ssh file",
        "scp file rdehome2:/tmp",
        "git push origin main",
        "cat ~/.ssh/config",
        "ssh",
        "ssh -o BatchMode=yes",
        "",
    ] {
        assert!(hosts(command).is_empty(), "{command}");
    }
}

#[test]
fn a_command_with_two_ssh_calls_lists_both_in_order() {
    assert_eq!(
        hosts("ssh rdehome1 ls; ssh rdehome2 ls"),
        vec!["rdehome1".to_string(), "rdehome2".to_string()]
    );
}

#[test]
fn a_remote_command_in_quotes_is_not_a_second_host() {
    assert_eq!(hosts("ssh rdehome1 'ssh rdehome2 ls'"), vec!["rdehome1"]);
}
