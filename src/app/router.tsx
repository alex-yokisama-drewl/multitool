import { createHashRouter, Navigate, RouterProvider } from "react-router-dom";
import { AppLayout } from "./AppLayout";
import { Dashboard } from "./Dashboard";
import { tools } from "@/tools/registry";

const router = createHashRouter([
  {
    path: "/",
    element: <AppLayout />,
    children: [
      { index: true, element: <Dashboard /> },
      ...tools.map((tool) => ({
        path: tool.route.replace(/^\//, ""),
        element: <tool.component />,
      })),
      { path: "*", element: <Navigate to="/" replace /> },
    ],
  },
]);

export function AppRouter() {
  return <RouterProvider router={router} />;
}
