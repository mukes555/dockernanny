// The standard setup for a React + TypeScript app, plus two rules that
// encode the house style: no nested ternaries, and files that stay small.
import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "node_modules", "src-tauri", "companion", ".tmp"] },
  {
    files: ["src/**/*.{ts,tsx}", "*.{js,ts}"],
    extends: [js.configs.recommended, ...tseslint.configs.recommended, reactHooks.configs.flat.recommended],
    languageOptions: { globals: globals.browser },
    rules: {
      "react-hooks/exhaustive-deps": "error",
      "max-lines": ["error", { max: 1000, skipBlankLines: true, skipComments: true }],
      // A choice among three reads better as named values or a small function.
      "no-nested-ternary": "error",
      // State that follows a prop is adjusted while rendering; effects only talk to the outside.
      "react-hooks/set-state-in-effect": "error",
      "react-hooks/purity": "error",
    },
  },
);
