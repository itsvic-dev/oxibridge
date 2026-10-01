{ self, pkgs }:
pkgs.nixosTest {
  name = "oxibridge-test";

  nodes.machine = { config, pkgs, ... }: {
    imports = [ self.nixosModules.oxibridge ];
    services.oxibridge = {
      enable = true;
      settings = {
        backends.src = {
          kind = "file";
          path = "${./src.txt}";
        };

        groups.test = {
          src.readonly = true;
          dst.writeonly = true;
        };
      };

      secretFiles = [
        (pkgs.writeText "secrets.yml" ''
          backends:
            dst:
              kind: file
              path: /var/lib/oxibridge/dst.txt
        '')
      ];
    };

    systemd.services.oxibridge.serviceConfig.StateDirectory = "oxibridge";
  };

  testScript = ''
    machine.wait_until_succeeds("sha256sum /var/lib/oxibridge/dst.txt | grep 2edc4d35d0fcdb59b8b88a0e6140e01f207bea18a52a11c3a55d06c6d409aac2", timeout=60)
  '';
}
