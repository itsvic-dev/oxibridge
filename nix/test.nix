{ self, pkgs }:
pkgs.nixosTest {
  name = "oxibridge-test";

  nodes.machine = { config, pkgs, ... }: {
    imports = [ self.nixosModules.oxibridge ];
    services.oxibridge = {
      enable = true;
      configFile = pkgs.writeText "config.yml" ''
        backends:
          src:
            # the "file" backend reads lines from a file and sends them as messages
            # and writes lines to a file when receiving messages
            kind: file
            token: ${./src.txt}

          dst:
            kind: file
            token: /var/lib/oxibridge/dst.txt

        groups:
          test:
            src:
              readonly: true
            dst:
              writeonly: true
      '';
    };

    systemd.services.oxibridge.serviceConfig.StateDirectory = "oxibridge";
  };

  testScript = ''
    import time
    machine.wait_for_unit("oxibridge.service")
    time.sleep(5)  # wait for messages to be processed

    # read dst.txt from service, expect known hash
    machine.succeed("sha256sum /var/lib/oxibridge/dst.txt | grep 2edc4d35d0fcdb59b8b88a0e6140e01f207bea18a52a11c3a55d06c6d409aac2")
  '';
}
