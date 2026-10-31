import { RouterProvider } from "react-router-dom";
import { router } from "./router.js";

export function App() {
  return <RouterProvider router={router} />;
}
