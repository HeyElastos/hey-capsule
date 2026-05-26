import { useEffect, useState } from "react";
import { detectShell } from "../lib/shell";

// Returns the active shell context: { name, version, hosted, source }.
// While the detection is in-flight, `loading` is true; once resolved,
// the result is stable for the page lifetime.
//
//   const shell = useShell();
//   if (shell.hostedByHeyHome) ...
//   if (shell.hostedByStockHome) ...
export const useShell = () => {
  const [state, setState] = useState({
    loading: true,
    name: null,
    version: null,
    hosted: false,
    hostedByHeyHome: false,
    hostedByStockHome: false,
  });

  useEffect(() => {
    let cancelled = false;
    detectShell().then((shell) => {
      if (cancelled) return;
      setState({
        loading: false,
        name: shell.name,
        version: shell.version,
        hosted: shell.hosted,
        hostedByHeyHome: shell.name === "hey-home",
        hostedByStockHome: shell.name === "home",
      });
    });
    return () => { cancelled = true; };
  }, []);

  return state;
};
