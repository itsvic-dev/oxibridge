{ oxibridge }:
{ config, lib, ... }:
with lib;
let
  cfg = config.services.oxibridge;
in
{
  options = {
    services.oxibridge = {
      enable = mkEnableOption "Oxibridge, a bot connecting multiple Telegram groups and Discord channels";
      configFile = mkOption {
        type = types.path;
        description = "Path to the configuration for Oxibridge.";
        default = "/run/oxibridge/config.yml";
      };
    };
  };

  config = mkIf cfg.enable {
    systemd.services.oxibridge = {
      wantedBy = [ "network-online.target" ];

      serviceConfig = {
        ExecStart = getExe oxibridge;
        Restart = "always";
      };

      environment = {
        CONFIG_FILE = cfg.configFile;
      };
    };
  };
}
