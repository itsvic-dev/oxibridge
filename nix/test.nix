{ self, pkgs }:
pkgs.nixosTest {
  name = "oxibridge-test";

  nodes.machine = { config, pkgs, ... }: {
    imports = [ self.nixosModules.oxibridge ];
    services.oxibridge = {
      enable = true;
      configFile = pkgs.replaceVars ./config-test.yml { "src" = ./src.txt; };
    };

    systemd.services.oxibridge.unitConfig.RuntimeDirectory = "oxibridge";
  };

  testScript = ''
    import time
    machine.wait_for_unit("oxibridge.service");
    time.sleep(5)
    # TODO: read dst.txt from service, compare against known good hash
  '';
}
