#!/usr/bin/env bash
# SPP VS Code eklentisini .vsix olarak paketler
set -e
cd "$(dirname "$0")"

# 1) Rust derleyicisini release build et (bin/spp.exe veya bin/spp üretir)
cargo build --release

mkdir -p bin
# 2) Binary'yi bin/ altına kopyala (platforma göre)
if [ -f target/release/spp ]; then cp target/release/spp bin/spp; fi
if [ -f target/release/spp.exe ]; then cp target/release/spp.exe bin/spp.exe; fi

# 3) Paket listesine binary'yi geri ekle (Windows için .cmd wrapper dahil)
node -e "
const fs=require('fs');
const p=JSON.parse(fs.readFileSync('package.json','utf8'));
const add=['bin/spp'+(process.platform==='win32'?'.exe':''), 'bin/spp.cmd'].filter((v,i,a)=>a.indexOf(v)===i);
p.files=[...new Set([...p.files, ...add.filter(f=>fs.existsSync(f))])];
fs.writeFileSync('package.json', JSON.stringify(p,null,2));
"

# 4) vsce ile paketle (publisher doğrulaması gerektirmez)
npx --yes @vscode/vsce package --allow-missing-repository

echo ">>> Hazır: $(ls -t *.vsix | head -1)"
