{ self, pkgs }:
pkgs.nixosTest {
  name = "oxibridge-test";

  nodes.machine = { config, pkgs, ... }: {
    imports = [ self.nixosModules.oxibridge ];
    services.oxibridge = {
      enable = true;
      configFile = pkgs.replaceVarsWith {
        src = ./config-test.yml;
        replacements = { "src" = ./src.txt; };
        name = "config.yml";
      };
    };
  };

  testScript = ''
    import time
    machine.wait_for_unit("oxibridge.service");
    time.sleep(5)
    # TODO: read dst.txt from service, compare against known good hash
  '';
}
