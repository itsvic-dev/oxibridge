{ self, pkgs }:
pkgs.nixosTest {
  name = "oxibridge-test";

  nodes.machine = { config, pkgs, ... }: {
    imports = [ self.nixosModules.oxibridge ];
    services.oxibridge = {
      enable = true;
      configFile = ./config-test.yml;
    };
  };

  testScript = ''
    machine.wait_for_unit("oxibridge.service");
    machine.succeed("systemctl is-active oxibridge.service");
  '';
}
