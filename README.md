# ZMydia

## Português

ZMydia é uma biblioteca pessoal de vídeos, local e privada. Ela organiza vídeos armazenados no seu computador em uma interface familiar para navegar, pesquisar, assistir e redescobrir seu acervo.

### O que o ZMydia oferece

- biblioteca visual com canais, tags, histórico, favoritos e playlists;
- recomendações locais baseadas nos seus hábitos, com controles de variedade e descoberta;
- player com progresso, legendas, capítulos, fila e modo Shorts;
- busca e estatísticas do acervo;
- downloads do YouTube por meio do ZMydia e da extensão para Firefox;
- organização segura de arquivos, detecção de duplicatas, quarentena e restauração;
- integração opcional com MusicBrainz e ListenBrainz;
- funcionamento local com banco SQLite e sem depender de uma conta na nuvem.

O servidor é acessível somente no próprio computador por padrão. As pastas começam em modo somente leitura, e alterações físicas nos arquivos exigem autorização explícita.

No Linux e no Windows, o instalador configura o ZMydia como um serviço em segundo plano do usuário. O Linux usa systemd e o Windows usa o Agendador de Tarefas para iniciar o ZMydia automaticamente no login e reiniciá-lo em caso de falha. Ambos incluem FFmpeg, ffprobe, yt-dlp, Deno e as demais dependências necessárias.

Em Debian, Ubuntu, Linux Mint e derivados, instale o pacote `.deb` e abra **ZMydia** pelo menu de aplicativos. O primeiro início ativa o serviço e o timer de backup para o usuário atual:

```bash
sudo apt install ./zmydia_*_amd64.deb
```

Em outras distribuições com systemd, use o instalador universal interativo. Ele baixa e verifica o pacote da release, instala sem `sudo` e apaga os arquivos temporários ao terminar:

```bash
sh zmydia-*-linux-x86_64-installer.sh
```

No Windows, execute `zmydia-*-windows-x86_64-installer.exe`. A desinstalação remove o serviço e os arquivos do aplicativo, preservando a biblioteca e a configuração do usuário.

No NixOS, o flake baixa o binário pronto da GitHub Release e fornece um módulo declarativo, sem recompilar a aplicação. Não use `nix profile install`: importe o módulo, habilite o serviço e atualize o input normalmente com `nix flake update zmydia`.

```nix
inputs.zmydia = {
  url = "github:alissonlima086/ZMydia";
  inputs.nixpkgs.follows = "nixpkgs";
};

# Em configuration.nix:
imports = [ inputs.zmydia.nixosModules.default ];
services.zmydia.enable = true;
```

## English

ZMydia is a private, local-first personal video library. It organizes videos stored on your computer into a familiar interface for browsing, searching, watching, and rediscovering your collection.

### What ZMydia offers

- a visual library with channels, tags, watch history, favorites, and playlists;
- local recommendations based on your viewing habits, with variety and discovery controls;
- a video player with progress tracking, subtitles, chapters, queues, and a Shorts mode;
- library-wide search and viewing statistics;
- YouTube downloads through ZMydia and its Firefox extension;
- safe file organization, duplicate detection, quarantine, and restore tools;
- optional MusicBrainz and ListenBrainz integration;
- local operation backed by SQLite, without requiring a cloud account.

By default, the server is available only on the local computer. Media folders start in read-only mode, and physical file changes require explicit authorization.

On Linux and Windows, the installer configures ZMydia as a per-user background service. Linux uses systemd, while Windows uses Task Scheduler to start ZMydia automatically at sign-in and restart it after failures. Both include FFmpeg, ffprobe, yt-dlp, Deno, and the other required runtime dependencies.

On Debian, Ubuntu, Linux Mint, and derivatives, install the `.deb` package and open **ZMydia** from the application menu. The first launch enables the service and backup timer for the current user:

```bash
sudo apt install ./zmydia_*_amd64.deb
```

On other distributions with systemd, use the interactive universal installer. It downloads and verifies the release package, installs without `sudo`, and removes temporary files when finished:

```bash
sh zmydia-*-linux-x86_64-installer.sh
```

On Windows, run `zmydia-*-windows-x86_64-installer.exe`. Uninstalling removes the background service and application files while preserving the user's library and configuration.

On NixOS, the flake downloads the prebuilt binary from the GitHub Release and provides a declarative module without rebuilding the application. Do not use `nix profile install`: import the module, enable the service, and update the input normally with `nix flake update zmydia`.

```nix
inputs.zmydia = {
  url = "github:alissonlima086/ZMydia";
  inputs.nixpkgs.follows = "nixpkgs";
};

# In configuration.nix:
imports = [ inputs.zmydia.nixosModules.default ];
services.zmydia.enable = true;
```

## License

MIT — see [LICENSE](LICENSE).
