use core::panic;
use libtaurus::{
    consensus::dag::{self, DAG},
    vertex::make_rand_vertex,
    wire::WireFormat,
    Vertex,
};
use std::{collections::HashMap, iter, sync::Arc};

macro_rules! edges {
    ([$($input:expr),*]) => {
        {
            [
                $(
                    {
                        let mut parts = $input.split(" -> ");
                        let left = parts.next().unwrap().trim();
                        let rights: Vec<&str> = parts.next().unwrap().split(',').map(|s| s.trim()).collect();
                        (left, rights)
                    }
                ),*
            ]
        }
    };
}

fn build_graph(edges: &[(&str, Vec<&str>)]) -> DAG {
    let mut vertices: HashMap<&str, Arc<Vertex>> = HashMap::new();
    vertices.insert(edges[0].0, Arc::new(Vertex::empty())); // first is genesis
    for (name, parent_names) in &edges[1..] {
        let parents = parent_names
            .into_iter()
            .map(|name| {
                vertices
                    .get(name)
                    .expect(&format!("couldn't find vertex {name}"))
                    .clone()
            })
            .collect::<Vec<_>>();
        vertices.insert(name, make_rand_vertex(&parents));
    }
    let mut dag = DAG::with_initial_vertices(
        dag::Config {
            genesis: vertices[edges[0].0].hash(),
            max_confidence: 9,
            max_count: 9,
        },
        iter::once(&vertices[edges[0].0]),
    )
    .unwrap();
    for (name, _) in &edges[1..] {
        dag.try_insert(&vertices[name]).unwrap();
    }
    dag
}

#[test]
fn basic_graph() {
    // Graph sketch:
    //      gen
    //      / \
    //    v00 v01
    //    |  X  |
    //    v10 v11
    //    |  X  |
    //    v20 v21
    let dag = build_graph(&edges!([
        "gen -> ",
        "v00 -> gen",
        "v01 -> gen",
        "v10 -> v00, v01",
        "v11 -> v00, v01",
        "v20 -> v10, v11",
        "v21 -> v10, v11"
    ]));
    todo!()
}

#[test]
fn old_record_query__delete_this_test() {
    // Set up test vertices and dag
    let gen = Arc::new(Vertex::empty());
    let a00 = make_rand_vertex([&gen]);
    let a01 = make_rand_vertex([&gen]);
    let a02 = make_rand_vertex([&gen]);

    // Create conflicting graphs b & c
    let b10 = make_rand_vertex([&a00, &a01]);
    let b11 = make_rand_vertex([&a00, &a02]);
    let b20 = make_rand_vertex([&b10]);
    let b21 = make_rand_vertex([&b10, &b11]);
    let b30 = make_rand_vertex([&b21, &b20]);
    let c10 = make_rand_vertex([&a01, &a00]); // Conflicts with b10
    let c11 = make_rand_vertex([&a02, &a00]); // Conflicts with b11
    let c20 = make_rand_vertex([&c10]);
    let c21 = make_rand_vertex([&c10, &c11]);
    let c30 = make_rand_vertex([&c21, &c20]);

    println!(":::: gen.hash() = {}", gen.hash());
    println!(":::: a00.hash() = {}", a00.hash());
    println!(":::: a01.hash() = {}", a01.hash());
    println!(":::: a02.hash() = {}", a02.hash());
    println!(":::: b10.hash() = {}", b10.hash());
    println!(":::: b11.hash() = {}", b11.hash());
    println!(":::: b20.hash() = {}", b20.hash());
    println!(":::: b21.hash() = {}", b21.hash());
    println!(":::: b30.hash() = {}", b30.hash());
    println!(":::: c10.hash() = {}", c10.hash());
    println!(":::: c11.hash() = {}", c11.hash());
    println!(":::: c20.hash() = {}", c20.hash());
    println!(":::: c21.hash() = {}", c21.hash());
    println!(":::: c30.hash() = {}", c30.hash());

    // Initialize new dag with genesis vertex
    let mut dag = dag::DAG::with_initial_vertices(
        dag::Config {
            genesis: gen.hash(),
            max_confidence: 9,
            max_count: 9,
        },
        [&gen],
    )
    .unwrap();

    // Insert everything into the DAG
    dag.try_insert(&a00).unwrap();
    dag.try_insert(&a01).unwrap();
    dag.try_insert(&a02).unwrap();
    dag.try_insert(&b10).unwrap();
    dag.try_insert(&b11).unwrap();
    dag.try_insert(&b20).unwrap();
    dag.try_insert(&b21).unwrap();
    dag.try_insert(&b30).unwrap();
    dag.try_insert(&c10).unwrap();
    dag.try_insert(&c11).unwrap();
    dag.try_insert(&c20).unwrap();
    dag.try_insert(&c21).unwrap();
    dag.try_insert(&c30).unwrap();

    // Test preferences based on insertion without chits
    assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);

    // Award chits to gen & "a" vertices, and confirm preferences don't change
    assert!(matches!(
        dag.record_query_result(&gen.hash(), &[]).unwrap_err(),
        dag::Error::AlreadyQueried
    ));
    dag.record_query_result(&a00.hash(), &[true]).unwrap();
    dag.record_query_result(&a01.hash(), &[true]).unwrap();
    dag.record_query_result(&a02.hash(), &[true]).unwrap();
    assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);

    // Award chit to c10 and confirm "c" subtree becomes partially preferred
    dag.record_query_result(&c10.hash(), &[true, true, true])
        .unwrap();
    assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);

    // Award chit to c11 to get full "c" subtree preference
    dag.record_query_result(&c11.hash(), &[true, true, true])
        .unwrap();
    assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);

    // Award chits to rest of "c" sub tree
    dag.record_query_result(&c20.hash(), &[true]).unwrap();
    dag.record_query_result(&c21.hash(), &[true, true, true])
        .unwrap();
    dag.record_query_result(&c30.hash(), &[true, true, true])
        .unwrap();
    assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);

    // Award chits to "b" sub tree, to equal "c" confidence. "c" sub tree should remain
    // preferred
    dag.record_query_result(&b10.hash(), &[true, true, true])
        .unwrap();
    dag.record_query_result(&b11.hash(), &[true, true, true])
        .unwrap();
    dag.record_query_result(&b20.hash(), &[true]).unwrap();
    dag.record_query_result(&b21.hash(), &[true, true, true])
        .unwrap();
    dag.record_query_result(&b30.hash(), &[true, true, true])
        .unwrap();
    assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), true);

    // Extend "b" subtree once more. Should be enough to flip "b" subtree into preference.
    let c40 = make_rand_vertex([&c30]);
    dag.try_insert(&c40).unwrap();
    dag.record_query_result(&c40.hash(), &[true]).unwrap();
    assert_eq!(dag.is_preferred(&gen.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a00.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a01.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&a02.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b10.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b11.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b20.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b21.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&b30.hash()).unwrap(), true);
    assert_eq!(dag.is_preferred(&c10.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&c11.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&c20.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&c21.hash()).unwrap(), false);
    assert_eq!(dag.is_preferred(&c30.hash()).unwrap(), false);
}
