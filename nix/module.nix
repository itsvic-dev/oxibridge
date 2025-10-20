self:
{ config, lib, pkgs, ... }:
with lib;
let
  cfg = config.services.oxibridge;
  package = self.packages.${pkgs.system}.default;
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
        Type = "simple";
        ExecStart = getExe package;
        Restart = "on-failure";
        DynamicUser = true;

        LoadCredential = "config-file:${cfg.configFile}";

        NoNewPrivileges = true;
        RemoveIPC = true;
        PrivateTmp = true;
        ProcSubset = "pid";
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectProc = "invisible";
        ProtectSystem = "full";
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        SystemCallArchitectures = "native";
        UMask = "0077";
      };

      environment.CONFIG_FILE = "%d/config-file";
    };
  };
}
