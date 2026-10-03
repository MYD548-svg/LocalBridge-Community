import release from "./identity.json";
import development from "./identity.dev.json";
declare const __LOCALBRIDGE_DEVELOPMENT__: boolean;
export default typeof __LOCALBRIDGE_DEVELOPMENT__ === "boolean" && __LOCALBRIDGE_DEVELOPMENT__ ? development : release;
