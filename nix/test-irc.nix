{ self, pkgs }:
let
  ircTester = pkgs.writers.writePython3Bin "irc-tester" { } ''
    import socket

    sock = socket.create_connection(("127.0.0.1", 6667))
    lines = sock.makefile("r", encoding="utf-8", newline="\r\n")


    def send(line):
        sock.sendall((line + "\r\n").encode())


    send("NICK tester")
    send("USER tester 0 * :tester")
    for line in lines:
        line = line.rstrip("\r\n")
        print(line, flush=True)
        if line.startswith("PING"):
            send("PONG" + line[4:])
        elif " 001 " in line:
            send("JOIN #test")
        elif line.startswith(":oxibridge!") and " JOIN " in line:
            send("PRIVMSG #test :hello from irc")
        elif line.endswith("PRIVMSG #test :file_backend: test"):
            break
  '';
in pkgs.testers.nixosTest {
  name = "oxibridge-irc-test";

  nodes.machine = { lib, ... }: {
    imports = [ self.nixosModules.oxibridge ];

    services.ngircd = {
      enable = true;
      config = ''
        [Global]
        Name = irc.test
        Ports = 6667

        [Options]
        DNS = no
        Ident = no
        PAM = no
      '';
    };

    services.oxibridge = {
      enable = true;
      settings = {
        backends = {
          src = {
            kind = "file";
            path = "${./src.txt}";
          };
          irc = {
            kind = "irc";
            server = "127.0.0.1";
            nickname = "oxibridge";
            use_tls = false;
          };
          dst = {
            kind = "file";
            path = "/var/lib/oxibridge/dst.txt";
          };
        };

        groups.test = {
          src.readonly = true;
          irc.channel = "#test";
          dst.writeonly = true;
        };
      };
    };

    systemd.services.oxibridge.wantedBy = lib.mkForce [ ];
    environment.systemPackages = [ ircTester ];
  };

  testScript = ''
    machine.wait_for_unit("ngircd.service")
    machine.wait_for_open_port(6667)
    machine.succeed("irc-tester > /tmp/tester.log 2>&1 &")
    machine.wait_until_succeeds("grep ' 001 ' /tmp/tester.log", timeout=30)

    machine.systemctl("start oxibridge.service")
    machine.wait_until_succeeds("grep -F 'PRIVMSG #test :file_backend: test' /tmp/tester.log", timeout=60)
    machine.wait_until_succeeds("grep -F '(test, irc) tester (@irc/tester): hello from irc' /var/lib/oxibridge/dst.txt", timeout=60)
  '';
}
