# QVault design context

React + Vite + Tailwind v4; Chinese local archive application. Both light and dark themes use existing tokens in `src/styles/main.css`.

Use the existing blue primary color, compact typography, 16px card radii, Lucide icons, and reusable Button/Card primitives. Archive task stages show confirmed completion, actual counters, interruption reasons and cooldowns; no invented percentage. Feed cards defer media loading and expand long content and replies on demand. Preserve keyboard access, narrow screen layout and reduced motion.

21st catalog references: [progress list](https://21st.dev/@sean0205/components/c-progress-5) and [post card](https://21st.dev/@shadcnui-blocks/components/card-06). Adapted to the existing application stack and tokens.
