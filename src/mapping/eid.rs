use super::SplinterMapping;

impl SplinterMapping {
    pub fn map_eid_server_to_proxy(&mut self, server_id: u64, server_eid: i32) -> i32 {
        if let Some(eid) = self.eids.get_by_right(&(server_id, server_eid)) {
            *eid
        } else {
            let new_eid = self.eid_gen.take_id() as i32;
            self.eids.insert(new_eid, (server_id, server_eid));
            new_eid
        }
    }
    pub fn map_eid_proxy_to_server(&mut self, proxy_eid: i32) -> anyhow::Result<(u64, i32)> {
        if let Some(server_eid_pair) = self.eids.get_by_left(&proxy_eid) {
            Ok(*server_eid_pair)
        } else {
            bail!("Could not find existing mapping for eid {}", proxy_eid);
        }
    }
}
