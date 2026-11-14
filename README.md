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

No Linux e no Windows, o instalador configura o ZMydia como um serviço em segundo plano do usuário. O Linux usa systemd e o Windows usa o Agendador de Tarefas para iniciar o ZMydia automaticamente no login e reiniciá-lo em caso de falha. Ambos incluem FFmpeg, ffprobe, yt-dlp e as demais dependências necessárias.

Para instalar no Linux:

```bash
chmod +x zmydia-*-linux-x86_64-installer.run
./zmydia-*-linux-x86_64-installer.run
```

No Windows, execute `zmydia-*-windows-x86_64-installer.exe`. A desinstalação remove o serviço e os arquivos do aplicativo, preservando a biblioteca e a configuração do usuário.

No NixOS, o flake baixa o binário pronto da GitHub Release e fornece `packages.default` e `nixosModules.default`, sem recompilar a aplicação. Atualizações são recebidas com `nix flake update` ou `nix profile upgrade`.

```nix
inputs.zmydia.url = "github:alissonlima086/ZMydia";

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

On Linux and Windows, the installer configures ZMydia as a per-user background service. Linux uses systemd, while Windows uses Task Scheduler to start ZMydia automatically at sign-in and restart it after failures. Both include FFmpeg, ffprobe, yt-dlp, and the other required runtime dependencies.

To install on Linux:

```bash
chmod +x zmydia-*-linux-x86_64-installer.run
./zmydia-*-linux-x86_64-installer.run
```

On Windows, run `zmydia-*-windows-x86_64-installer.exe`. Uninstalling removes the background service and application files while preserving the user's library and configuration.

On NixOS, the flake downloads the prebuilt binary from the GitHub Release and provides `packages.default` and `nixosModules.default` without rebuilding the application. Updates are received through `nix flake update` or `nix profile upgrade`.

```nix
inputs.zmydia.url = "github:alissonlima086/ZMydia";

# In configuration.nix:
imports = [ inputs.zmydia.nixosModules.default ];
services.zmydia.enable = true;
```

## License

MIT — see [LICENSE](LICENSE).
