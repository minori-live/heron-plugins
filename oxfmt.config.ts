import { defineConfig } from "oxfmt"

export default defineConfig({
  semi: false,
  trailingComma: "none",
  printWidth: 100,
  sortPackageJson: false,
  ignorePatterns: ["target/**", "coverage/**", "dist/**", "node_modules/**", "third_party/**"]
})
