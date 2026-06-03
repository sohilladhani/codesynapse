{
  description = "Code intelligence MCP server for AI coding assistants";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }:
    let
      version = "0.1.0";

      binaries = {
        "x86_64-linux" = {
          url = "https://github.com/sohilladhani/codesynapse/releases/download/v${version}/codesynapse-linux-x86_64";
          hash = "sha256-yOVy74z6nsyH1rMxexu2VengmzhoiEhACQIz/sMsmsg=";
        };
        "aarch64-darwin" = {
          url = "https://github.com/sohilladhani/codesynapse/releases/download/v${version}/codesynapse-macos-aarch64";
          hash = "sha256-emgj2m+wUXF70hMlVh49d5YWDXIOI/bPt7pl1s7WUxA=";
        };
      };

      supportedSystems = builtins.attrNames binaries;
      forEachSystem = f: nixpkgs.lib.genAttrs supportedSystems (system: f system);
    in {
      packages = forEachSystem (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          bin = binaries.${system};
        in {
          default = pkgs.stdenv.mkDerivation {
            pname = "codesynapse";
            inherit version;

            src = pkgs.fetchurl {
              inherit (bin) url hash;
            };

            dontUnpack = true;

            installPhase = ''
              mkdir -p $out/bin
              cp $src $out/bin/codesynapse
              chmod +x $out/bin/codesynapse
            '';

            meta = with pkgs.lib; {
              description = "Code intelligence MCP server for AI coding assistants";
              homepage = "https://github.com/sohilladhani/codesynapse";
              license = licenses.mit;
              platforms = supportedSystems;
              mainProgram = "codesynapse";
            };
          };
        }
      );

      apps = forEachSystem (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/codesynapse";
        };
      });
    };
}
