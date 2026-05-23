import { Link } from "react-router-dom";
import { toolCategories, tools } from "@/tools/registry";

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

  const sections = toolCategories
    .map((category) => ({
      category,
      tools: tools.filter((tool) => tool.category === category.id),
    }))
    .filter((section) => section.tools.length > 0);

  return (
    <div className="space-y-8">
      {sections.map((section) => (
        <section key={section.category.id}>
          <div className="flex items-center gap-3">
            <h2 className="text-sm font-medium uppercase tracking-wide text-muted-foreground">
              {section.category.label}
            </h2>
            <hr className="flex-1 border-border" />
          </div>
          <ul className="mt-4 grid gap-3 sm:grid-cols-2">
            {section.tools.map((tool) => (
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
        </section>
      ))}
    </div>
  );
}
