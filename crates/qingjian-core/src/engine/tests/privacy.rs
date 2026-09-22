//! 私密输入：不学、不记；离开后恢复。

use super::*;

fn pick(engine: &mut Engine, input: &str, text: &str) {
    engine.set_input(input);
    let candidate = engine
        .query()
        .unwrap()
        .candidates
        .items
        .into_iter()
        .find(|c| c.text == text && c.kind == CandidateKind::Chinese)
        .unwrap();
    engine.commit(&candidate);
}

#[test]
fn private_input_learns_nothing_and_logs_nothing() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut engine = engine()
        .with_learner(Box::new(CountingLearner(HashMap::new())))
        .with_input_logger(Box::new(MemoryLogger(log.clone())));
    pick(&mut engine, "kaifa", "开放");
    assert_eq!(engine.learner().weight("开放"), 1);
    let logged_before = log.lock().unwrap().len();
    assert!(logged_before > 0);
    let order = |engine: &mut Engine| -> Vec<String> {
        engine.set_input("kaifa");
        let items = engine.query().unwrap().candidates.items;
        engine.clear();
        items.into_iter().map(|c| c.text).collect()
    };
    let ranked = order(&mut engine);

    engine.set_private(true);
    assert!(engine.is_private());
    // 读照常：已学的仍参与排序，私密前后候选顺序一样
    assert_eq!(order(&mut engine), ranked);
    pick(&mut engine, "kaifa", "开发");
    pick(&mut engine, "xian", "先");
    // 删掉重选也不撤销：私密期间没记过
    engine.note_backspace();
    pick(&mut engine, "kaifa", "开放");
    assert_eq!(engine.learner().weight("开发"), 0);
    assert_eq!(engine.learner().weight("先"), 0);
    assert_eq!(engine.learner().weight("开放"), 1);
    assert_eq!(log.lock().unwrap().len(), logged_before);

    engine.set_private(false);
    pick(&mut engine, "kaifa", "开发");
    assert_eq!(engine.learner().weight("开发"), 1);
    assert!(log.lock().unwrap().len() > logged_before);
}
