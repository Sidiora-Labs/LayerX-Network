use std::path::PathBuf;
use std::time::{SystemTime,UNIX_EPOCH};
use layerx_agentd::budget::{NativeBudgetRuntime,NativeBudgetScope,NativeBudgetReconciliation};
use layerx_agentd::capability::NativeReservation;
use layerx_agentd::outbox::{Outbox,SubmissionState};
use layerx_agentd::sign::VerifiedSubmission;
use layerx_agentd::store::{Store,TenantId};
use super::{checked,Result,fixture::Fixture};

pub fn now_ms()->Result<u64> {
    checked(u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis()))
}

pub struct Session {
    pub scope:NativeBudgetScope,
    pub tenant:TenantId,
    pub store:Store,
    pub outbox:Outbox,
    pub runtime:NativeBudgetRuntime,
    path:PathBuf,
}

impl Session {
    pub fn open(fixture:&mut Fixture,id:u8)->Result<Self> {
        let scope=fixture.create(id)?;
        let path=fixture.directory.join(format!("store-{id}"));
        let mut value=Self {scope,tenant:checked(TenantId::new("native-budget-owner"))?,
            store:Store::open(&path)?,outbox:Outbox::default(),runtime:NativeBudgetRuntime::new(fixture.authority.clone()),path};
        let proof=value.reconcile(fixture)?;
        assert_eq!(proof.spent(),0);assert_eq!(proof.remaining(),100);
        Ok(value)
    }

    pub fn reconcile(&mut self,fixture:&mut Fixture)->Result<NativeBudgetReconciliation> {
        checked(self.runtime.reconcile(&mut self.store,&self.tenant,&mut fixture.client,
            &fixture.registry,&self.scope,&mut self.outbox))
    }

    pub fn enqueue(&mut self,fixture:&mut Fixture,id:u8,recipient:[u8;32])->Result<Vec<u8>> {
        let signed=fixture.spend(&self.scope,id,25,recipient)?;
        let exact=signed.exact_bytes().to_vec();
        let proof=self.reconcile(fixture)?;
        let reservation=reservation(&signed,&fixture.registry)?;
        checked(self.runtime.reserve(&self.tenant,self.scope.binding.budget_id,reservation,
            now_ms()?,proof.observed_sequence()))?;
        checked(self.outbox.enqueue(&mut self.store,self.tenant.clone(),[id;32],signed))?;
        self.assert_accounting(0,25)?;
        Ok(exact)
    }

    pub fn transition(&mut self,id:u8,state:SubmissionState)->Result<()> {
        checked(self.outbox.transition(&mut self.store,[id;32],state,
            "actual native Budget write stage before restart",None))?;
        if state==SubmissionState::Submitted {
            checked(self.runtime.mark_unknown(&self.tenant,self.scope.binding.budget_id,[id;32]))?;
        }
        Ok(())
    }

    pub fn restart(self,fixture:&Fixture)->Result<Self> {
        let ids:Vec<_>=self.outbox.statuses().into_iter().map(|value| value.submission_id).collect();
        let scope=self.scope.clone();let tenant=self.tenant.clone();let path=self.path.clone();
        drop(self);
        let store=Store::open(&path)?;
        let mut outbox=Outbox::default();
        for id in ids {checked(outbox.restore(&store,tenant.clone(),id))?;}
        Ok(Self {scope,tenant,store,outbox,runtime:NativeBudgetRuntime::new(fixture.authority.clone()),path})
    }

    pub fn assert_accounting(&self,spent:u128,held:u128)->Result<()> {
        let ceiling=self.runtime.ceiling(&self.tenant,self.scope.binding.budget_id).ok_or("recovery ceiling missing")?;
        let snapshot=checked(ceiling.snapshot())?;
        assert!(snapshot.reconciled);assert_eq!(snapshot.maximum,100);
        assert_eq!(snapshot.consumed,spent);assert_eq!(snapshot.held,held);
        assert_eq!(snapshot.reservations,usize::from(held!=0));
        Ok(())
    }
}

pub fn reservation(signed:&VerifiedSubmission,registry:&layerx_types::payload::ModuleRegistry)->Result<NativeReservation> {
    let activity=checked(layerx_wire::activity::decode_signed(signed.exact_bytes(),registry))?;
    assert_eq!(activity.activity_type().value(),0x0003_0006);
    let amount=u128::from_be_bytes(activity.payload()[66..82].try_into()?);
    Ok(NativeReservation {id:signed.idempotency_key(),expected_activity_id:signed.activity_id(),amount,
        expiry_ms:activity.timestamp_bound().not_after,unknown:false})
}

fn assert_actions(session:&Session,id:u8,queued:bool,unknown:bool)->Result<()> {
    let statuses=session.outbox.statuses();
    assert_eq!(statuses.len(),1);
    assert_eq!(statuses.iter().filter(|value|value.state==SubmissionState::Queued).count(),usize::from(queued));
    assert_eq!(statuses.iter().filter(|value|value.state==SubmissionState::Unknown).count(),usize::from(unknown));
    assert_eq!(statuses[0].submission_id,[id;32]);
    Ok(())
}

pub fn recovery(fixture:&mut Fixture)->Result<()> {
    for (offset,stage) in [SubmissionState::Queued,SubmissionState::Submitted,
        SubmissionState::Acknowledged,SubmissionState::Unknown,SubmissionState::Executed,SubmissionState::Failed]
        .into_iter().enumerate() {
        let offset=checked(u8::try_from(offset))?;
        let mut session=Session::open(fixture,0x90+offset)?;
        let id=0x50+offset;
        let recipient=if stage==SubmissionState::Failed {[0xee;32]} else {session.scope.binding.owner_account};
        let exact=session.enqueue(fixture,id,recipient)?;
        if stage!=SubmissionState::Queued {session.transition(id,SubmissionState::Submitted)?;}
        if matches!(stage,SubmissionState::Queued|SubmissionState::Submitted) {
            session=session.restart(fixture)?;session.reconcile(fixture)?;
            assert_actions(&session,id,stage==SubmissionState::Queued,stage==SubmissionState::Submitted)?;
            session.assert_accounting(0,25)?;
        } else {
            if stage==SubmissionState::Unknown {fixture.drop_response(&exact)?;session.transition(id,SubmissionState::Unknown)?;}
            else {fixture.submit(&exact)?;session.transition(id,SubmissionState::Acknowledged)?;}
            let activity=session.outbox.status([id;32]).ok_or("submission missing")?.activity_id;
            let (receipt,header)=fixture.receipt(activity)?;
            let success=checked(layerx_wire::receipt::decode(&receipt))?.result_code()==0;
            assert_eq!(success,stage!=SubmissionState::Failed);
            session=session.restart(fixture)?;
            assert!(session.reconcile(fixture).is_err(),"unfinalized execution must not authorize recovery writes");
            fixture.finalize(&header)?;
            session.reconcile(fixture)?;
            let expected=if success {SubmissionState::Executed} else {SubmissionState::Failed};
            assert_eq!(session.outbox.status([id;32]).ok_or("terminal missing")?.state,expected);
            session.assert_accounting(if success {25} else {0},0)?;
            assert_actions(&session,id,false,false)?;
            let transitions=session.outbox.status([id;32]).ok_or("terminal missing")?.transitions.clone();
            session=session.restart(fixture)?;session.reconcile(fixture)?;
            assert_eq!(session.outbox.status([id;32]).ok_or("terminal missing")?.transitions,transitions);
        }
        assert_eq!(checked(session.outbox.exact_signed_bytes([id;32]))?,exact);
    }
    Ok(())
}

pub fn unknown(fixture:&mut Fixture)->Result<()> {
    let mut session=Session::open(fixture,0xa0)?;
    let id=0x61;
    let exact=session.enqueue(fixture,id,session.scope.binding.owner_account)?;
    session.transition(id,SubmissionState::Submitted)?;
    session=session.restart(fixture)?;session.reconcile(fixture)?;
    assert_actions(&session,id,false,true)?;session.assert_accounting(0,25)?;
    let before=now_ms()?;
    assert!(checked(session.outbox.begin_native_retry(&mut session.store,[id;32],before))?);
    assert!(!checked(session.outbox.begin_native_retry(&mut session.store,[id;32],before))?);
    session=session.restart(fixture)?;session.reconcile(fixture)?;
    assert!(!checked(session.outbox.begin_native_retry(&mut session.store,[id;32],before))?);
    let restored=checked(session.outbox.exact_signed_bytes([id;32]))?.to_vec();
    assert_eq!(restored,exact);
    fixture.drop_response(&restored)?;
    let activity=session.outbox.status([id;32]).ok_or("unknown missing")?.activity_id;
    let (first_receipt,header)=fixture.receipt(activity)?;
    session.assert_accounting(0,25)?;
    assert_eq!(session.outbox.status([id;32]).ok_or("unknown missing")?.state,SubmissionState::Unknown);
    fixture.drop_response(&restored)?;
    let (repeated_receipt,repeated_header)=fixture.receipt(activity)?;
    assert_eq!(first_receipt,repeated_receipt);assert_eq!(header,repeated_header);
    session=session.restart(fixture)?;
    assert!(session.reconcile(fixture).is_err());
    fixture.finalize(&header)?;
    let evidence=session.reconcile(fixture)?;
    assert_eq!(evidence.outcomes().iter().filter(|value|value.activity_id()==activity).count(),1);
    session.assert_accounting(25,0)?;
    session=session.restart(fixture)?;session.reconcile(fixture)?;session.assert_accounting(25,0)?;
    assert_eq!(checked(session.outbox.exact_signed_bytes([id;32]))?,exact);
    Ok(())
}
