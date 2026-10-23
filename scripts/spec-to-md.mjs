// Converts the HTML build spec into docs/SPEC.md (simple, spec-specific conversion).
import { readFileSync, writeFileSync } from "node:fs";
const html = readFileSync(process.argv[2], "utf8");
let body = html.split("<body>")[1].split("</body>")[0];
const dec = (s) => s.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&amp;/g, "&");
body = body.replace(/<pre><code>([\s\S]*?)<\/code><\/pre>/g, (_, c) => "\n```\n" + dec(c) + "\n```\n");
body = body.replace(/<table>([\s\S]*?)<\/table>/g, (_, t) => {
  const rows = [...t.matchAll(/<tr>([\s\S]*?)<\/tr>/g)].map((r) => [...r[1].matchAll(/<t[hd]>([\s\S]*?)<\/t[hd]>/g)].map((c) => c[1].trim()));
  const out = rows.map((r) => "| " + r.join(" | ") + " |");
  out.splice(1, 0, "| " + rows[0].map(() => "---").join(" | ") + " |");
  return "\n" + out.join("\n") + "\n";
});
body = body.replace(/<h1>(.*?)<\/h1>/g, "# $1\n").replace(/<h2[^>]*>(.*?)<\/h2>/g, "\n## $1\n");
body = body.replace(/<div class="meta">(.*?)<\/div>/g, "_$1_\n");
body = body.replace(/<ol>([\s\S]*?)<\/ol>/g, (_, l) => { let i = 0; return "\n" + l.replace(/<li>([\s\S]*?)<\/li>/g, (_, x) => `${++i}. ${x.trim()}`).replace(/\n\s+/g, "\n") + "\n"; });
body = body.replace(/<ul>([\s\S]*?)<\/ul>/g, (_, l) => "\n" + l.replace(/<li>([\s\S]*?)<\/li>/g, (_, x) => `- ${x.trim()}`).replace(/\n\s+/g, "\n") + "\n");
body = body.replace(/<p>([\s\S]*?)<\/p>/g, "\n$1\n");
body = body.replace(/<strong>(.*?)<\/strong>/g, "**$1**").replace(/<code>(.*?)<\/code>/g, "`$1`").replace(/<a href="(.*?)">(.*?)<\/a>/g, "[$2]($1)");
body = dec(body).replace(/\n{3,}/g, "\n\n").trim() + "\n";
writeFileSync(process.argv[3], body);
