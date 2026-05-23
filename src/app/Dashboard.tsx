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
          <ul className="mt-4 grid grid-cols-3 gap-3 sm:grid-cols-4 md:grid-cols-5">
            {section.tools.map((tool) => (
              <li key={tool.id}>
                <Link
                  to={tool.route}
                  data-tile-color={tool.color}
                  style={{
                    backgroundColor: `var(--tile-${tool.color})`,
                    color: `var(--tile-${tool.color}-fg)`,
                  }}
                  className="block aspect-square overflow-hidden rounded-lg shadow-sm transition-all duration-150 hover:-translate-y-0.5 hover:shadow-lg"
                >
                  <div className="flex h-full flex-col p-4">
                    <div className="text-sm font-medium">{tool.name}</div>
                    <div className="mt-10 line-clamp-3 text-xs opacity-70">
                      {tool.description}
                    </div>
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
