const vscode = require('vscode');
const cp = require('child_process');
const fs = require('fs');
const path = require('path');

const output = vscode.window.createOutputChannel('SPP');

function candidates() {
  const configured = vscode.workspace.getConfiguration('spp').get('executablePath');
  const result = [];
  if (configured) result.push(configured);

  const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  if (root) {
    if (process.platform === 'win32') {
      result.push(path.join(root, 'target', 'release', 'spp.exe'));
      result.push(path.join(root, 'target', 'debug', 'spp.exe'));
    } else {
      result.push(path.join(root, 'target', 'release', 'spp'));
      result.push(path.join(root, 'target', 'debug', 'spp'));
    }
  }

  const bundled = process.platform === 'win32' ? 'spp.exe' : 'spp';
  result.push(path.join(__dirname, 'bin', bundled));
  result.push('spp');
  return [...new Set(result)];
}

function resolveCompiler() {
  for (const candidate of candidates()) {
    if (candidate === 'spp' || fs.existsSync(candidate)) return candidate;
  }
  return null;
}

function currentFile() {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== 'spp') {
    vscode.window.showWarningMessage('SPP: Open a .spp file first.');
    return null;
  }
  return editor.document;
}

async function runCompiler(args, cwd) {
  const compiler = resolveCompiler();
  if (!compiler) {
    const action = await vscode.window.showErrorMessage(
      'SPP compiler not found. Build the project with cargo build --release or configure spp.executablePath.',
      'Open Settings'
    );
    if (action) await vscode.commands.executeCommand('workbench.action.openSettings', '@ext:spp-project.spp-language-support executablePath');
    return false;
  }

  output.clear();
  output.appendLine(`SPP ${compiler} ${args.map(a => JSON.stringify(a)).join(' ')}`);
  output.appendLine('');
  output.show(true);

  return await new Promise((resolve) => {
    const child = cp.spawn(compiler, args, {
      cwd,
      windowsHide: true,
      shell: false,
      env: { ...process.env, SPP_EXTENSION: '1' }
    });
    child.stdout.on('data', chunk => output.append(chunk.toString()));
    child.stderr.on('data', chunk => output.append(chunk.toString()));
    child.on('error', err => {
      output.appendLine(`\nSPP process error: ${err.message}`);
      vscode.window.showErrorMessage(`SPP could not start: ${err.message}`);
      resolve(false);
    });
    child.on('close', code => {
      output.appendLine(`\n[SPP exit ${code ?? 'unknown'}]`);
      if (code === 0) vscode.window.setStatusBarMessage('SPP ✓', 1500);
      else vscode.window.showErrorMessage(`SPP command failed with exit code ${code}. See the SPP output channel.`);
      resolve(code === 0);
    });
  });
}

async function withCurrentFile(fn) {
  const doc = currentFile();
  if (!doc) return;
  await doc.save();
  await fn(doc.uri.fsPath, path.dirname(doc.uri.fsPath));
}

function runCurrent() { return withCurrentFile((file, cwd) => runCompiler(['run', file], cwd)); }
function checkCurrent() { return withCurrentFile((file, cwd) => runCompiler(['check', file], cwd)); }
function buildCurrent() { return withCurrentFile((file, cwd) => runCompiler(['build', file], cwd)); }
function benchmarkCurrent() { return withCurrentFile((file, cwd) => runCompiler(['benchmark', file, '5'], cwd)); }

function openTerminal() {
  const compiler = resolveCompiler();
  const cwd = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  const terminal = vscode.window.createTerminal({
    name: 'SPP Terminal',
    cwd,
    env: compiler && compiler !== 'spp'
      ? { PATH: `${path.dirname(compiler)}${path.delimiter}${process.env.PATH || ''}` }
      : undefined
  });
  terminal.show(true);
}

async function verify() {
  const compiler = resolveCompiler();
  if (!compiler) {
    vscode.window.showErrorMessage('SPP compiler not found. Build it with cargo build --release or configure spp.executablePath.');
    return;
  }
  const cwd = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || process.cwd();
  await runCompiler(['--version'], cwd);
}

function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand('spp.run', runCurrent),
    vscode.commands.registerCommand('spp.check', checkCurrent),
    vscode.commands.registerCommand('spp.build', buildCurrent),
    vscode.commands.registerCommand('spp.benchmark', benchmarkCurrent),
    vscode.commands.registerCommand('spp.terminal', openTerminal),
    vscode.commands.registerCommand('spp.verify', verify),
    output
  );
}

function deactivate() {}
module.exports = { activate, deactivate };
