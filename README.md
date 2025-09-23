# load-balancer

A set of asynchronous load balancers for Rust, supporting multiple strategies:

- **IP-based**: bind clients to system IP addresses.
- **Threshold**: limit retries based on errors.
- **Limit**: restrict usage per interval.
- **Random**: pick entries randomly.
- **Simple**: round-robin sequential allocation.

## Examples

```rust
#[tokio::main]
async fn main() {
    // Each node can be used at most 2 times, interval 1 second
    let lb = LimitLoadBalancer::new(vec![(2, "node 1"), (2, "node 2")]);
    let start = tokio::time::Instant::now();

    for _ in 0..8 {
        let node = lb.alloc().await.unwrap();

        println!("{}s Allocated node: {}", start.elapsed().as_secs(), node);
    }

    println!("------------------------------");

    // Each node can be used at most 4 times, interval 5 second
    let lb = LimitLoadBalancer::new_interval(
        vec![(3, "node 1"), (1, "node 2")],
        Duration::from_secs(5),
    );

    let start = tokio::time::Instant::now();

    for _ in 0..8 {
        let node = lb.alloc().await.unwrap();

        println!("{}s Allocated node: {}", start.elapsed().as_secs(), node);
    }
}
```

### Output

```
0s Allocated node: node 1
0s Allocated node: node 1
0s Allocated node: node 2
0s Allocated node: node 2
1s Allocated node: node 1
1s Allocated node: node 1     
1s Allocated node: node 2     
1s Allocated node: node 2     
------------------------------
0s Allocated node: node 1     
0s Allocated node: node 1     
0s Allocated node: node 1     
0s Allocated node: node 2     
5s Allocated node: node 1
5s Allocated node: node 1
5s Allocated node: node 1
5s Allocated node: node 2
```