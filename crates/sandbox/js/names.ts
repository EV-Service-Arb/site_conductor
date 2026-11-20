// The top-level declarations of cells: each line of stdin a cell's code as a JSON string, each line of
// stdout what later cells see of it.
import ts from "typescript";

function names(code: string) {
  const file = ts.createSourceFile("cell.ts", code, ts.ScriptTarget.Latest, false, ts.ScriptKind.TS);
  const names: { name: string; type: boolean; exported: boolean }[] = [];
  const bound = (b: ts.BindingName, exported: boolean) => {
    if (ts.isIdentifier(b)) names.push({ name: b.text, type: false, exported });
    else for (const e of b.elements) if (!ts.isOmittedExpression(e)) bound(e.name, exported);
  };
  for (const s of file.statements) {
    const has = (k: ts.SyntaxKind) => ts.canHaveModifiers(s) && (ts.getModifiers(s) ?? []).some((m) => m.kind === k);
    if (has(ts.SyntaxKind.DeclareKeyword)) continue; // nothing at run time to hand on
    const exported = has(ts.SyntaxKind.ExportKeyword);
    if (ts.isVariableStatement(s)) for (const d of s.declarationList.declarations) bound(d.name, exported);
    else if ((ts.isFunctionDeclaration(s) || ts.isClassDeclaration(s) || ts.isEnumDeclaration(s)) && s.name) names.push({ name: s.name.text, type: false, exported });
    else if (ts.isInterfaceDeclaration(s) || ts.isTypeAliasDeclaration(s)) names.push({ name: s.name.text, type: true, exported });
  }
  return names;
}

const out = new TextEncoder();
let buffered = "";
for await (const chunk of Deno.stdin.readable.pipeThrough(new TextDecoderStream())) {
  buffered += chunk;
  for (let at = buffered.indexOf("\n"); at >= 0; at = buffered.indexOf("\n")) {
    const code: string = JSON.parse(buffered.slice(0, at));
    buffered = buffered.slice(at + 1);
    await Deno.stdout.write(out.encode(JSON.stringify(names(code)) + "\n"));
  }
}
