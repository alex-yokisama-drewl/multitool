import js from "@eslint/js";
import tseslint from "typescript-eslint";
import react from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import prettier from "eslint-config-prettier";
import globals from "globals";

export default tseslint.config(
  {
    ignores: ["dist", "node_modules", "src-tauri/target", "src-tauri/gen"],
  },
  // Base JS rules apply to all JS/TS files.
  js.configs.recommended,
  // Type-checked TS rules only apply where we have a tsconfig project.
  {
    files: ["**/*.{ts,tsx}"],
    extends: [
      ...tseslint.configs.recommendedTypeChecked,
      ...tseslint.configs.stylisticTypeChecked,
    ],
    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
      parserOptions: {
        project: ["./tsconfig.json", "./tsconfig.node.json"],
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {
      react,
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },
    settings: {
      react: { version: "detect" },
    },
    rules: {
      ...react.configs.recommended.rules,
      ...react.configs["jsx-runtime"].rules,
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": [
        "warn",
        { allowConstantExport: true },
      ],
      // Per CLAUDE.md: no `any` without an inline justification comment.
      // Suppressions must be opt-in via
      // `// eslint-disable-next-line @typescript-eslint/no-explicit-any -- <why>`.
      "@typescript-eslint/no-explicit-any": "error",
    },
  },
  // Node-side config files (no TS project, no typed rules).
  {
    files: ["*.config.{js,ts}", "eslint.config.js"],
    languageOptions: {
      globals: globals.node,
    },
  },
  prettier,
);
