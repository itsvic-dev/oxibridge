{ config, lib, pkgs, ... }:
with lib;
let
  cfg = config.services.oxibridge;
  package = pkgs.callPackage ./package.nix { };
in {
  options = {
    services.oxibridge = {
      enable = mkEnableOption
        "Oxibridge, a bot connecting multiple Telegram groups and Discord channels";
      configFile = mkOption {
        type = types.path;
        description = "Path to the configuration for Oxibridge.";
        default = "/run/oxibridge/config.yml";
      };
    };
  };

  config = mkIf cfg.enable {
    systemd.services.oxibridge = {
      after = [ "network.target" "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];

      serviceConfig = {
        ExecStart = getExe package;
        Restart = "on-failure";
        User = "oxibridge";
        Group = "oxibridge";
      };

      environment = { CONFIG_FILE = cfg.configFile; };
    };

    users.users.oxibridge = {
      isSystemUser = true;
      group = "oxibridge";
    };
    users.groups.oxibridge = { };
  };
}
