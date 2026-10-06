use std::path::Path;

const USAGE: &str = "uso: cargo run -p xtask -- web-bench audio-diff --base URL [--out FILE]";

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .filter(|v| !v.starts_with("--"))
}

fn usage() -> i32 {
    eprintln!("{USAGE}");
    2
}

pub fn main(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("audio-diff") => audio_diff(args),
        Some("behavior-diff") => behavior_diff(args),
        _ => usage(),
    }
}

fn audio_diff(args: &[String]) -> i32 {
    let Some(base) = flag(args, "--base") else {
        return usage();
    };
    let out = flag(args, "--out").unwrap_or("docs/verification/fase3/web-ui-sounds.json");
    let run = || -> Result<serde_json::Value, String> {
        let argv = crate::mcp::default_command();
        let args = argv.iter().map(String::as_str).collect::<Vec<_>>();
        let mut client = crate::mcp::Client::spawn(&args)?;
        let mut page = client.open_page(&format!(
            "{}/xtask/web/fixtures/sounds/parity.html?web=on",
            base.trim_end_matches('/')
        ))?;
        let value=page.eval("async () => { await window.fixtureReady; if (!window.__comandosReady || typeof window.__comandosRenderCue !== 'function') throw new Error('actual WASM render hook did not mount'); const js = await import('/assets/uisfx/uisfx-0.4.0.js'); const rows=[]; for (const cue of ['open','close','complete','success','level-up','notification','error','warning']) { const baseline=js.renderRecipe(js.createRecipe('arcade',cue),48000).left; const candidate=window.__comandosRenderCue(cue,48000); const render=async samples=>{ const ctx=new OfflineAudioContext(1,samples.length,48000); const buffer=ctx.createBuffer(1,samples.length,48000); buffer.getChannelData(0).set(samples); const source=ctx.createBufferSource(); source.buffer=buffer; source.connect(ctx.destination); source.start(); return (await ctx.startRendering()).getChannelData(0); }; const a=await render(baseline),b=await render(candidate); let max=0; for(let i=0;i<Math.min(a.length,b.length);i++) max=Math.max(max,Math.abs(a[i]-b[i])); rows.push({cue,max_abs_diff:max,same_duration:a.length===b.length,baseline_samples:a.length,candidate_samples:b.length}); } return {wasm_url:window.fixtureWasmUrl,cues:rows}; }")?;
        page.close()?;
        Ok(value)
    };
    let (value, exit) = match run() {
        Ok(mut value) => {
            let passed = value
                .get("cues")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|rows| {
                    rows.len() == 8
                        && rows.iter().all(|r| {
                            r.get("same_duration").and_then(serde_json::Value::as_bool)
                                == Some(true)
                                && r.get("max_abs_diff")
                                    .and_then(serde_json::Value::as_f64)
                                    .is_some_and(|v| v.is_finite() && v <= 1e-3)
                        })
                });
            value["base"] = base.into();
            value["status"] = if passed { "pass" } else { "fail" }.into();
            (value, if passed { 0 } else { 1 })
        }
        Err(error) => (
            serde_json::json!({"base":base,"status":"error","reason":error}),
            1,
        ),
    };
    if let Some(parent) = Path::new(out).parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("error: {}: {e}", parent.display());
        return 1;
    }
    match serde_json::to_string_pretty(&value)
        .map(|s| s + "\n")
        .and_then(|s| std::fs::write(out, s).map_err(serde_json::Error::io))
    {
        Ok(()) => {
            println!("audio-diff {}: wrote {out}", value["status"]);
            exit
        }
        Err(e) => {
            eprintln!("error: {out}: {e}");
            1
        }
    }
}

fn behavior_diff(args: &[String]) -> i32 {
    let Some(base) = flag(args, "--base") else {
        return usage();
    };
    let out = flag(args, "--out").unwrap_or("docs/verification/fase3/web-b5.json");
    let run = || -> Result<serde_json::Value, String> {
        let argv = crate::mcp::default_command();
        let argv = argv.iter().map(String::as_str).collect::<Vec<_>>();
        let mut client = crate::mcp::Client::spawn(&argv)?;
        let url = format!(
            "{}/xtask/web/fixtures/sounds/parity.html",
            base.trim_end_matches('/')
        );
        let mut page = client.open_page(&format!("{url}?web=off"))?;
        let script = "async () => {await window.fixtureReady; const {runBehavior}=await import('/xtask/web/fixtures/sounds/behavior.mjs'); return await runBehavior();}";
        let baseline = page.eval(script)?;
        page.navigate(&format!("{url}?web=on"))?;
        let candidate = page.eval(script)?;
        let proof=page.eval("() => ({wasm_url:window.fixtureWasmUrl,ready:window.__comandosReady,actions:window.fixtureApiCalls})")?;
        let negative=page.eval("async () => {const {runBehavior}=await import('/xtask/web/fixtures/sounds/behavior.mjs');const old=WorkspaceLayout.moveTab;WorkspaceLayout.moveTab=()=>({groups:[]});try {await runBehavior();return {detected:false};} catch(error){return {detected:true,error:error.message};} finally {WorkspaceLayout.moveTab=old;}}")?;
        let oracle = page.eval(
            "async () => await(await fetch('/xtask/web/fixtures/sounds/oracle.json')).json()",
        )?;
        let push_url=format!("{}/xtask/web/fixtures/sounds/push-ui.html",base.trim_end_matches('/'));
        let push_script="async () => {await window.fixtureReady;const {checkPushHandlers}=await import('/xtask/web/fixtures/sounds/push-handlers.mjs');return await checkPushHandlers();}";
        page.navigate(&format!("{push_url}?web=off"))?;let push_baseline=page.eval(push_script)?;
        page.navigate(&format!("{push_url}?web=on"))?;let push_candidate=page.eval(push_script)?;
        page.close()?;
        let passes = push_candidate==push_baseline && candidate == baseline
            && candidate == oracle
            && negative["detected"] == true
            && proof["ready"] == true
            && proof["wasm_url"]
                .as_str()
                .is_some_and(|s| s.ends_with(".wasm"));
        Ok(
            serde_json::json!({"status":if passes{"pass"}else{"fail"},"baseline":"original JavaScript","candidate":"actual built WASM","proof":proof,"node_original_oracle_matches":candidate==oracle,"remote_original_matches":candidate==baseline,"negative_calibration":negative,"cases":candidate,"push_dom_handlers":{"matches_original":push_candidate==push_baseline,"result":push_candidate},"prior_visual_evidence":{"status":"invalid","reason":"Prior c8009c5/8d1f3ea fixtures were static HTML; they did not load or act through WASM. Prior capture artifacts retained but no longer count as B4/B5 acceptance."}}),
        )
    };
    let value = run().unwrap_or_else(|error| serde_json::json!({"status":"error","reason":error}));
    let status = if value["status"] == "pass" { 0 } else { 1 };
    if let Some(parent) = Path::new(out).parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("{e}");
        return 1;
    }
    if let Err(e) = std::fs::write(out, format!("{value:#}\n")) {
        eprintln!("{out}: {e}");
        return 1;
    }
    println!("behavior-diff {}: wrote {out}", value["status"]);
    status
}

#[cfg(test)]
mod tests {
    use super::flag;

    #[test]
    fn flag_rejects_missing_values() {
        let args = ["audio-diff", "--base", "--out", "x"]
            .iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>();
        assert_eq!(flag(&args, "--base"), None);
        assert_eq!(flag(&args, "--out"), Some("x"));
    }
}
