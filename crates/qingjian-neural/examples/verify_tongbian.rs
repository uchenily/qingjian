//! 验证含章·通变（P2C）能否把 `bushuhenfangbian` 纠正成「部署很方便」。
//!
//! 用法：cargo run --release -p qingjian-neural --example verify_tongbian -- data/models/hanzhang-tongbian/hanzhang-tongbian-small.qjm

use std::path::PathBuf;
use std::time::Instant;

use qingjian_neural::{CharScorer, P2c};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path: PathBuf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "data/models/hanzhang-tongbian/hanzhang-tongbian-small.qjm".into())
        .into();

    let t = Instant::now();
    let scorer = CharScorer::load(&path)?;
    println!(
        "加载 {} 耗时 {} ms（name={:?}，layers={}，vocab={}）",
        path.display(),
        t.elapsed().as_millis(),
        scorer.metadata().map(|m| m.name.as_str()),
        scorer.model().config().n_layer,
        scorer.vocab().len(),
    );
    println!("字表有 <sep>：{}", scorer.vocab().sep().is_some());

    let decoder = P2c::new(scorer.model(), scorer.vocab())
        .ok_or("字表没有 <sep>，不是通变模型")?;

    // 1) 直接生成：拼音 → 汉字（beam search，不经词图）
    for keys in ["bushuhenfangbian", "yongdockerbushuhenfangbian", "woxiangxuexirust"] {
        let t = Instant::now();
        let cands = decoder.convert(keys, 5, 32)?;
        println!("\n[生成] {} （{} ms）", keys, t.elapsed().as_millis());
        for c in cands.iter().take(5) {
            println!("   {:.3}  {}", c.score, c.text);
        }
    }

    // 2) 教师强制打分：对几个候选句子打 log P(汉字|拼音)，看「部署很方便」是否高于「不是很方便」
    let keys = "bushuhenfangbian";
    let texts = ["部署很方便", "不是很方便", "不熟很方便", "部署很方便吗"];
    let scores = scorer.score_p2c(keys, &texts)?;
    println!("\n[打分] 拼音 = {}", keys);
    for (text, score) in texts.iter().zip(scores) {
        println!("   {:.3}  {}", score, text);
    }

    Ok(())
}
