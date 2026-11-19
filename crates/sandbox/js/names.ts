// The top-level declarations of a cell, read off stdin: what later cells see of it.
import ts from "typescript";

const code = await new Response(Deno.stdin.readable).text();
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
console.log(JSON.stringify(names));
