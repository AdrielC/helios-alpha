import DefaultTheme from "vitepress/theme";
import "@fontsource-variable/archivo";
import "@fontsource-variable/recursive";
import "./styles.css";

import AtlasHome from "./components/AtlasHome.vue";

export default {
  extends: DefaultTheme,
  enhanceApp({ app }) {
    app.component("AtlasHome", AtlasHome);
  },
};
