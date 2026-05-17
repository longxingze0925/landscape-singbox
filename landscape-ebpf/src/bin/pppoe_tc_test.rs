use std::time::Duration;

use landscape_ebpf::pppoe;
use tokio::time::sleep;

#[tokio::main]
async fn main() {
    // ping -4 -I ens6 -M do -s 1472 DESKTOP-D4MDN4E.lan
    // ping -6 -I ens6 -M do -s 1444 DESKTOP-D4MDN4E.lan
    let (notice_tx, notice_rx) = tokio::sync::oneshot::channel::<()>();
    let notise = pppoe::pppoe_tc::create_pppoe_tc_ebpf_3(5, 0x2233, 1490).await.unwrap();
    println!("结束 ebpf pppoe 创建");
    sleep(Duration::from_secs(20)).await;
    println!("应该结束了");
    notise.send(notice_tx).unwrap();
    println!("发送结束请求");
    let _ = notice_rx.await;
    println!("结束");
}
