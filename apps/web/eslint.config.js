import js from '@eslint/js'
import { defineConfig, globalIgnores } from 'eslint/config'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import { plugin as shadcn } from '@shadcn/lint'
import tseslint from 'typescript-eslint'

export default defineConfig([
  globalIgnores(['dist', 'node_modules']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [js.configs.recommended, tseslint.configs.recommended, reactHooks.configs.flat.recommended],
    languageOptions: {
      ecmaVersion: 'latest',
      globals: globals.browser,
    },
    // Plugin wiring only. Design rules are intentionally not enabled in this scaffold.
    plugins: { shadcn },
  },
  {
    files: ['vite.config.ts', 'eslint.config.js'],
    languageOptions: { globals: globals.node },
  },
])
