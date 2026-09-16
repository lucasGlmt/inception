import * as fs from "node:fs";
import * as path from "node:path";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Trace,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

function serverCommand(): string {
  const configured = vscode.workspace.getConfiguration("lux").get<string>("server.path", "").trim();
  if (configured) {
    return configured;
  }

  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    let directory = folder.uri.fsPath;
    for (;;) {
      const candidate = path.join(directory, "target", "debug", process.platform === "win32" ? "lux-lsp.exe" : "lux-lsp");
      if (fs.existsSync(candidate)) {
        return candidate;
      }
      const parent = path.dirname(directory);
      if (parent === directory) {
        break;
      }
      directory = parent;
    }
  }
  return "lux-lsp";
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const serverOptions: ServerOptions = { command: serverCommand(), args: [] };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "lux" }, { scheme: "untitled", language: "lux" }],
    synchronize: { fileEvents: vscode.workspace.createFileSystemWatcher("**/*.lux") },
  };
  client = new LanguageClient("lux", "Lux Language Server", serverOptions, clientOptions);
  const trace = vscode.workspace.getConfiguration("lux").get<string>("trace.server", "off");
  client.setTrace(trace === "verbose" ? Trace.Verbose : trace === "messages" ? Trace.Messages : Trace.Off);
  context.subscriptions.push(client);
  await client.start();
}

export async function deactivate(): Promise<void> {
  await client?.stop();
}
