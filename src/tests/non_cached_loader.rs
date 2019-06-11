use std::thread;

use futures::{executor, future, TryFutureExt as _};

use super::*;

#[test]
fn assert_kinds() {
    fn _assert_send<T: Send>() {}
    fn _assert_sync<T: Sync>() {}
    fn _assert_clone<T: Clone>() {}
    _assert_send::<Loader<u32, u32, u32, Batcher>>();
    _assert_sync::<Loader<u32, u32, u32, Batcher>>();
    _assert_clone::<Loader<u32, u32, u32, Batcher>>();
}

#[test]
fn smoke() {
    let mut rt = executor::LocalPool::new();

    let loader = Loader::new(Batcher::new(2));

    let v = future::try_join3(loader.load(1), loader.load(2), loader.load(3));
    assert_eq!((10, 20, 30), rt.run_until(v).unwrap());

    let v = loader.load_many(vec![10, 20, 30]);
    assert_eq!(vec![100, 200, 300], rt.run_until(v).unwrap());

    let loader_ref = &loader;
    {
        let v = future::try_join(loader_ref.load(1), loader_ref.load(2));
        assert_eq!((10, 20), rt.run_until(v).unwrap());
    }
    {
        let v1 = loader_ref.load(3).and_then(|v| loader_ref.load(v));
        let v2 = loader_ref.load(4).and_then(|v| loader_ref.load(v));
        assert_eq!((300, 400), rt.run_until(future::try_join(v1, v2)).unwrap());
    }
}

#[test]
fn drop_loader() {
    let mut rt = executor::LocalPool::new();

    let all = {
        let loader = Loader::new(Batcher::new(10));
        let v1 = loader.load(1);
        let v2 = loader.load(2);
        drop(loader);
        future::try_join(v1, v2)
    };
    assert_eq!((10, 20), rt.run_until(all).unwrap());
}

#[test]
fn dispatch_partial_batch() {
    let mut rt = executor::LocalPool::new();

    let loader = Loader::new(Batcher::new(10));
    let all = future::try_join(loader.load(1), loader.load(2));

    assert_eq!((10, 20), rt.run_until(all).unwrap());
}

#[test]
fn nested_load() {
    let mut rt = executor::LocalPool::new();

    let loader = Loader::new(Batcher::new(2));
    let v1 = loader.load(3).and_then(|v| loader.load(v));
    let v2 = loader.load(4).and_then(|v| loader.load(v));
    assert_eq!((300, 400), rt.run_until(future::try_join(v1, v2)).unwrap());
}

#[test]
fn nested_load_many() {
    let mut rt = executor::LocalPool::new();

    let loader = Loader::new(Batcher::new(2));
    let v1 = loader
        .load(3)
        .and_then(|v| loader.load_many(vec![v, v + 1, v + 2]));
    let v2 = loader
        .load(4)
        .and_then(|v| loader.load_many(vec![v, v + 1, v + 2]));
    let expected = (vec![300, 310, 320], vec![400, 410, 420]);
    assert_eq!(expected, rt.run_until(future::try_join(v1, v2)).unwrap());
}

#[test]
fn test_batch_fn_error() {
    let mut rt = executor::LocalPool::new();

    let loader = Loader::<i32, i32, MyError, BadBatcher>::new(BadBatcher);
    let v1 = rt.run_until(loader.load(1));
    assert_eq!(LoadError::BatchFn(MyError::Unknown), v1.err().unwrap());
}

#[test]
fn test_result_val() {
    let mut rt = executor::LocalPool::new();

    let loader = Loader::<i32, Result<i32, ValueError>, MyError, BadBatcher>::new(BadBatcher);
    let v1 = rt.run_until(loader.load_many(vec![1, 2]));
    assert_eq!(vec![Err(ValueError::NotEven), Ok(20)], v1.unwrap());
}

#[test]
fn test_batch_call_seq() {
    let mut rt = executor::LocalPool::new();

    // batch size = 2, value will be (batch_fn call seq, v * 10)
    let loader = Loader::<i32, (usize, i32), (), Batcher>::new(Batcher::new(2));
    let v1 = loader.load(1);
    let v2 = loader.load(2);
    let v3 = loader.load(3);
    let v4 = loader.load(4);
    let v5 = loader.load(1);
    let v6 = loader.load(2);

    //v1 and v2 should be in first batch
    assert_eq!(
        ((1, 10), (1, 20)),
        rt.run_until(future::try_join(v1, v2)).unwrap(),
    );
    //v3 and v4 should be in second batch
    assert_eq!(
        ((2, 30), (2, 40)),
        rt.run_until(future::try_join(v3, v4)).unwrap(),
    );
    //v5 and v6 should be be in third batch
    assert_eq!(
        ((3, 10), (3, 20)),
        rt.run_until(future::try_join(v5, v6)).unwrap(),
    );
}

#[test]
fn pass_to_thread() {
    let loader = Loader::new(Batcher::new(4));

    let l = loader.clone();
    let h1 = thread::spawn(move || {
        let mut rt = executor::LocalPool::new();
        let all = future::try_join(l.load(1), l.load(2));
        assert_eq!((10, 20), rt.run_until(all).unwrap());
    });

    let l2 = loader.clone();
    let h2 = thread::spawn(move || {
        let mut rt = executor::LocalPool::new();
        let all = future::try_join(l2.load(1), l2.load(2));
        assert_eq!((10, 20), rt.run_until(all).unwrap());
    });

    let _ = h1.join();
    let _ = h2.join();
}

#[test]
fn test_run_by_threadpool() {
    let mut rt = executor::ThreadPool::new().unwrap();

    let loader = Loader::new(Batcher::new(10));
    let loader2 = loader.clone();
    let v1 = loader
        .clone()
        .load(3)
        .and_then(move |v| loader.load_many(vec![v, v + 1, v + 2]));
    let v2 = loader2
        .clone()
        .load(4)
        .and_then(move |v| loader2.load_many(vec![v, v + 1, v + 2]));
    assert_eq!(
        (vec![300, 310, 320], vec![400, 410, 420]),
        rt.run(future::try_join(v1, v2)).unwrap(),
    );
}

#[test]
fn test_values_length() {
    let mut rt = executor::LocalPool::new();

    let loader = Loader::<i32, (), (), BadBatcher>::new(BadBatcher);
    let v1 = loader.load(1);
    let v2 = loader.load(2);
    let (r1, r2) = rt.run_until(future::join(v1, v2));
    assert_eq!(
        LoadError::UnequalKeyValueSize {
            key_count: 2,
            value_count: 0,
        },
        r1.unwrap_err(),
    );
    assert_eq!(
        LoadError::UnequalKeyValueSize {
            key_count: 2,
            value_count: 0,
        },
        r2.unwrap_err(),
    );
}
