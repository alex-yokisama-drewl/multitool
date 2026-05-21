import { Link } from "react-router-dom";
import { tools } from "@/tools/registry";

export function Dashboard() {
  if (tools.length === 0) {
    return (
      <div className="rounded-lg border border-dashed border-border p-10 text-center">
        <h1 className="text-xl font-semibold">No tools yet</h1>
        <p className="mt-2 text-sm text-muted-foreground">
          Add a tool under <code>src/tools/</code> and register it in{" "}
          <code>src/tools/registry.ts</code>.
        </p>
      </div>
    );
  }

  return (
    <div>
      <h1 className="text-xl font-semibold">Tools</h1>
      <ul className="mt-6 grid gap-3 sm:grid-cols-2">
        {tools.map((tool) => (
          <li key={tool.id}>
            <Link
              to={tool.route}
              className="block rounded-lg border border-border p-4 hover:bg-accent"
            >
              <div className="text-sm font-medium">{tool.name}</div>
              <div className="mt-1 text-xs text-muted-foreground">
                {tool.description}
              </div>
            </Link>
          </li>
        ))}
      </ul>
    </div>
  );
}
