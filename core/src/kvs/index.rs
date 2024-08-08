use crate::cnf::{EXPORT_BATCH_SIZE, NORMAL_FETCH_SIZE};
use crate::dbs::Options;
use crate::err::Error;
use crate::kvs::ds::TransactionFactory;
use crate::kvs::LockType::Optimistic;
use crate::kvs::{LockType, TransactionType};
use crate::sql::statements::DefineIndexStatement;
use crate::sql::{Id, Thing, Value};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task;
use tokio::task::JoinHandle;
use crate::ctx::Context;
use crate::idx::index::IndexOperation;

enum BuildingStatus {
	Initiated,
	Building(usize),
	Error(Error),
	Done,
}

pub(crate) struct IndexBuilder {
	tf: TransactionFactory,
	indexes: DashMap<DefineIndexStatement, (Arc<Building>, JoinHandle<()>)>,
}

impl IndexBuilder {
	pub(super) fn new(tf: TransactionFactory) -> Self {
		Self {
			tf,
			indexes: Default::default(),
		}
	}

	pub(crate) fn build(&mut self, opt: &Options, ix: DefineIndexStatement) -> Result<(), Error> {
		match self.indexes.entry(ix) {
			Entry::Occupied(e) => {
				// If the building is currently running we return error
				if !e.get().1.is_finished() {
					panic!("Is running") // TODO replace by error
				}
			}
			Entry::Vacant(e) => {
				// No index is currently building, we can start building it
				let building = Arc::new(Building::new(self.tf.clone(), opt, e.key())?);
				let b = building.clone();
				let jh = task::spawn(async move { if let Err(err) = b.compute().await {} });
				e.insert((building, jh));
			}
		}
		Ok(())
	}

	pub(crate) async fn append(
		&self,
		ix: &DefineIndexStatement,
		old_values: Option<Vec<Value>>,
		new_values: Option<Vec<Value>>,
		rid: Thing,
	) -> Result<(), Error> {
		if let Some(e) = self.indexes.get(ix) {
			e.value()
				.0
				.append(Appending {
					old_values,
					new_values,
					id: rid.id,
				})
				.await;
			Ok(())
		} else {
			panic!("Not running") // TODO replace by error
		}
	}
}

struct Appending {
	old_values: Option<Vec<Value>>,
	new_values: Option<Vec<Value>>,
	id: Id,
}

struct Building {
	ctx: Context<'_>,
	tf: TransactionFactory,
	ns: String,
	db: String,
	tb: String,
	status: Arc<Mutex<BuildingStatus>>,
	// Should be stored on a temporary table
	appended: Arc<Mutex<VecDeque<Appending>>>,
	// Index barrier
	index_barrier: Arc<Mutex<()>>,
}

impl Building {
	fn new(
		ctx: &Context<'_>,
		tf: TransactionFactory,
		opt: &Options,
		ix: &DefineIndexStatement,
	) -> Result<Self, Error> {
		Ok(Self {
			tf,
			ns: opt.ns()?.to_string(),
			db: opt.db()?.to_string(),
			tb: ix.what.to_string(),
			status: Arc::new(Mutex::new(BuildingStatus::Initiated)),
			appended: Default::default(),
			index_barrier: Default::default(),
		})
	}

	async fn set_status(&self, status: BuildingStatus) {
		*self.status.lock().await = status;
	}

	async fn append(&self, a: Appending) {
		self.appended.lock().await.push_back(a);
	}

	async fn compute(&self) -> Result<(), Error> {
		self.set_status(BuildingStatus::Building(0)).await;
		// First iteration, we index every keys
		let beg = crate::key::thing::prefix(&self.ns, &self.db, &self.tb);
		let end = crate::key::thing::suffix(&self.ns, &self.db, &self.tb);
		let mut next = Some(beg..end);
		let mut count = 0;
		while let Some(rng) = next {
			let tx = self.tf.transaction(TransactionType::Write, LockType::Optimistic).await?;
			// Get the next batch of records
			let batch = tx.batch(rng, *EXPORT_BATCH_SIZE, true).await?;
			// Set the next scan range
			next = batch.next;
			// Check there are records
			if batch.values.is_empty() {
				break;
			}
			// Index the records
			for (_, v) in batch.values.into_iter() {
				count += 1;
				self.set_status(BuildingStatus::Building(count)).await;
			}
			tx.commit().await?;
		}
		// Second iteration, we index/remove any keys that has been added or removed since the initial indexing
		loop {
			let mut batch = self.appended.lock().await;
			let drain = batch.drain(0..*NORMAL_FETCH_SIZE as usize);
			if drain.len() == 0 {
				// LOCK INDEXING
				// if self.appended is still empty, we are done
				// UNLOCK INDEXING
				break;
			}
			let tx = self.tf.transaction(TransactionType::Write, Optimistic).await?;
			for a in drain {
				IndexOperation::new()
				match id {
					Appending::Updated(id) => {
						let key = crate::key::thing::new(&self.ns, &self.db, &self.tb, &id);
						let val = tx.get(key).await?;
						todo!("Add to index");
					}
					Appending::Removed(id) => {
						todo!("Remove from index")
					}
				}
				count += 1;
				self.set_status(BuildingStatus::Building(count)).await;
			}
			tx.commit().await?;
		}
		self.set_status(BuildingStatus::Done).await;
		Ok(())
	}
}