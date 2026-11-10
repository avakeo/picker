import { mount } from "svelte";
import App from "./App.svelte";
import "./styles.css";

const target = document.getElementById("app");

if (!target) {
  throw new Error("Picker UI mount target was not found");
}

export default mount(App, { target });
