# Third-party notices

Chemifree's independently authored code is MIT licensed. Dependencies retain their own licenses.

- The source in `apps/web/src/components/ui/`, the initial theme in `apps/web/src/index.css`, and the utility entry in `apps/web/src/lib/utils.ts` were generated from the official [shadcn/ui registry](https://github.com/shadcn-ui/ui) with shadcn CLI **4.22.1** (`select.tsx` with 4.23.0), Base UI / Nova. Copyright (c) 2023 shadcn, MIT. [License](apps/web/public/licenses/shadcn-MIT.txt).
- The generated components use [Base UI](https://github.com/mui/base-ui). Copyright (c) 2019 Material-UI SAS, MIT. [License](apps/web/public/licenses/base-ui-MIT.txt).
- [Lucide](https://github.com/lucide-icons/lucide) icons are used under ISC, with MIT notices for icons derived from Feather. [Full notices](apps/web/public/licenses/lucide-ISC-MIT.txt).
- The unmodified [Geist](https://github.com/vercel/geist-font) variable font is supplied through `@fontsource-variable/geist` under SIL Open Font License 1.1. [Copyright and license](apps/web/public/licenses/geist-OFL-1.1.txt).

These license files are also copied to the web distribution at `/licenses/`. npm and Cargo dependencies are pinned in their lockfiles. No proprietary vendor code, chemical engines, or model datasets are distributed by this scaffold.
