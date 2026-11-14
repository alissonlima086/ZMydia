# Third-party components

The installable ZMydia distributions include these unmodified command-line tools:

- **FFmpeg and ffprobe**, from the FFmpeg project, distributed under the LGPL 2.1 or later. The pipeline uses the LGPL static builds published by BtbN/FFmpeg-Builds. Source and build scripts: <https://github.com/FFmpeg/FFmpeg> and <https://github.com/BtbN/FFmpeg-Builds>.
- **yt-dlp**, distributed under The Unlicense. Source: <https://github.com/yt-dlp/yt-dlp>.
- **curl**, distributed under the curl license. Source: <https://github.com/curl/curl>. Windows binaries come from <https://curl.se/windows/>; Linux static builds come from <https://github.com/stunnel/static-curl>.

The release pipeline verifies every downloaded runtime archive against a checksum published with the corresponding upstream release. License texts shipped by the upstream archives are copied into the `licenses/` directory when available.

These programs remain separate executables invoked by ZMydia. They are not linked into the ZMydia binary.

The Windows installer is built with **NSIS 3.11**. NSIS and its included compression modules are distributed under their respective zlib/libpng, bzip2, and Common Public License 1.0 terms. License details and source: <https://github.com/NSIS-Dev/nsis>.
